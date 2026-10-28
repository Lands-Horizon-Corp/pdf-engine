#!/usr/bin/env node
// Parity with Chrome: render the same HTML with headless Chrome and with pdf-engine, draw both PDFs with the
// same rasteriser and score how close they look. See ../docs/parity.md.
//
//   node parity.mjs [--check] [--update-baseline] [--only <text>]
//
// Needs: `cargo build --release --example parity_diff` and the release engine binary (this script builds both),
// and puppeteer with a Chrome (installed in ../benchmark).
import { execFileSync } from 'node:child_process';
import { createRequire } from 'node:module';
import { copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync, rmSync } from 'node:fs';
import { basename, join, resolve } from 'node:path';

const here = import.meta.dirname;
const root = resolve(here, '../..');
const out = join(here, 'out');
const bench = createRequire(join(root, '.public/benchmark/package.json'));
const args = process.argv.slice(2);
const flag = (f) => args.includes(f);
const only = args.includes('--only') ? args[args.indexOf('--only') + 1] : '';

// A case passes at or above PASS, is close at or above CLOSE, and fails below it (or on a different page count).
const PASS = 90;
const CLOSE = 70;

// cases/*.html are the right-to-left pages; every folder in cases/ is a category of its own (see gen-cases.py).
const caseDirs = existsSync(join(here, 'cases')) ? readdirSync(join(here, 'cases'), { withFileTypes: true }).filter((d) => d.isDirectory()).map((d) => d.name).sort() : [];
const sources = [
  ['css', join(root, 'tests/fixtures/css')],
  ['probe', join(root, 'tests/fixtures/probes')],
  ['bidi', join(here, 'cases')],
  ...caseDirs.map((d) => [d, join(here, 'cases', d)]),
];
const FOLDER_LABELS = { 'page-count': 'pagination: page count', 'forced-breaks': 'pagination: forced breaks', 'break-inside': 'pagination: break-inside avoid', 'table-pagination': 'pagination: tables', 'image-boundaries': 'pagination: images', 'flex-grid-pages': 'pagination: flex and grid', 'fixed-header-footer': 'pagination: headers and footers', 'complex-tables': 'complex tables', 'bulleted-lists': 'bulleted lists', 'text': 'text' };
const cases = sources.flatMap(([group, dir]) =>
  existsSync(dir)
    ? readdirSync(dir).filter((f) => f.endsWith('.html')).sort().map((f) => ({ group, name: `${group}/${basename(f, '.html')}`, file: join(dir, f) }))
    : [],
).filter((c) => c.name.includes(only));

const run = (cmd, a, opts = {}) => execFileSync(cmd, a, { cwd: root, stdio: ['ignore', 'pipe', 'inherit'], maxBuffer: 1 << 28, ...opts });
console.error('building the engine and the diff tool ...');
run('cargo', ['build', '--release', '--quiet']);
run('cargo', ['build', '--release', '--quiet', '--example', 'parity_diff']);
const diffTool = join(root, 'target/release/examples/parity_diff');
const engineBin = join(root, 'target/release/pdf-engine');

rmSync(out, { recursive: true, force: true });
mkdirSync(join(out, 'pdf'), { recursive: true });
mkdirSync(join(out, 'png'), { recursive: true });

const { PDFEngine } = bench('@lands-horizon-corp/pdf-engine');
const puppeteer = bench('puppeteer');
const engine = new PDFEngine({ binary: engineBin });
const browser = await puppeteer.launch({ headless: 'shell' });
const chromeVersion = (await browser.version()).replace(/^.*\//, '');
const page = await browser.newPage();

// The categories the demo page shows: a probe is filed by what it tests, a whole-document fixture by its kind.
const categoryOf = (c) => {
  if (c.group === 'bidi') return 'right-to-left';
  if (c.group === 'css') return 'whole documents';
  if (FOLDER_LABELS[c.group]) return FOLDER_LABELS[c.group];
  const n = c.name.replace('probe/', '');
  const rules = [[/^(border|outline)/, 'borders'], [/^(bg|span_background)/, 'backgrounds'], [/^(color|opacity)/, 'colour and opacity'], [/^(inline|float)/, 'inline and floats'], [/^table/, 'tables'], [/^(flex|grid)/, 'flexbox and grid'], [/^(transform)/, 'transforms'], [/^(position|z_index)/, 'positioning'], [/^img/, 'images'], [/^(multicolumn)/, 'columns'], [/^(box|block|css_var|nth|before|media|page)/, 'box model and selectors']];
  return rules.find(([re]) => re.test(n))?.[1] ?? 'other';
};
const results = [];
for (const c of cases) {
  const slug = c.name.replace('/', '__');
  // Chrome renders a page with no doctype in quirks mode (it ignores the first element's top margin, for one thing);
  // the engine is a standards-mode renderer, so both get a doctype and the comparison is standards mode for both.
  const raw = readFileSync(c.file, 'utf8');
  const html = /^\s*<!doctype/i.test(raw) ? raw : '<!doctype html>' + raw;
  const r = { name: c.name, group: c.group, category: categoryOf(c), image: `parity/${slug}.png` };
  try {
    await page.setContent(html, { waitUntil: 'load' });
    const chrome = await page.pdf({ format: 'Letter', printBackground: true, preferCSSPageSize: true, margin: { top: 0, right: 0, bottom: 0, left: 0 } });
    writeFileSync(join(out, 'pdf', `${slug}.chrome.pdf`), chrome);
    const ours = await engine.generate({ template: html });
    writeFileSync(join(out, 'pdf', `${slug}.engine.pdf`), ours);
    const d = JSON.parse(run(diffTool, [join(out, 'pdf', `${slug}.chrome.pdf`), join(out, 'pdf', `${slug}.engine.pdf`), join(out, 'png', `${slug}.png`)]));
    if (d.error) throw new Error(d.error);
    r.pagesChrome = d.pagesReference;
    r.pagesEngine = d.pagesCandidate;
    r.pixel = +d.similarity.toFixed(2); // exact pixels after a 1 px shift: strict, text drift counts
    r.layout = +d.layout.toFixed(2); // mean colour per 6 mm block: does it look the same
    r.pageCountMatch = d.pagesReference === d.pagesCandidate;
    r.score = r.pageCountMatch ? r.layout : 0;
    r.verdict = !r.pageCountMatch ? 'fail' : r.score >= PASS ? 'pass' : r.score >= CLOSE ? 'close' : 'fail';
    if (!r.pageCountMatch) r.why = `page count: Chrome ${d.pagesReference}, pdf-engine ${d.pagesCandidate}`;
  } catch (e) {
    r.score = 0;
    r.verdict = 'error';
    r.why = String(e.message || e).slice(0, 300);
  }
  results.push(r);
  console.error(`${r.verdict.padEnd(5)} ${String(r.score).padStart(6)}  ${r.name}${r.why ? '  (' + r.why + ')' : ''}`);
}
await browser.close();
await engine.close();

// ---- the numbers -------------------------------------------------------------------------------
const mean = (xs) => (xs.length ? xs.reduce((a, b) => a + b, 0) / xs.length : 0);
const count = (v) => results.filter((r) => r.verdict === v).length;
const baselinePath = join(here, 'baseline.json');
const baseline = existsSync(baselinePath) ? JSON.parse(readFileSync(baselinePath, 'utf8')) : null;
const REGRESSION = 2; // points a case may lose against the baseline before it is called a regression
const regressions = baseline
  ? results.filter((r) => baseline.cases?.[r.name] !== undefined && r.score < baseline.cases[r.name] - REGRESSION).map((r) => ({ name: r.name, was: baseline.cases[r.name], now: r.score }))
  : [];

const score = +mean(results.map((r) => r.score)).toFixed(2);
const passRate = +((100 * count('pass')) / Math.max(results.length, 1)).toFixed(1);
// HEALTHY: score at least 85, no errors, nothing got worse than the baseline.
// DEGRADED: score 60 to 85, or something got worse, or a case errored. BROKEN: below 60.
const health = score < 60 ? 'BROKEN' : score < 85 || regressions.length || count('error') ? 'DEGRADED' : 'HEALTHY';

const groups = {};
for (const r of results) (groups[r.group] ||= []).push(r);
const cats = {};
for (const r of results) (cats[r.category] ||= []).push(r);
const categories = Object.entries(cats).map(([name, rs]) => ({ name, cases: rs.length, score: +mean(rs.map((r) => r.score)).toFixed(2), pass: rs.filter((r) => r.verdict === 'pass').length, close: rs.filter((r) => r.verdict === 'close').length, fail: rs.filter((r) => r.verdict === 'fail' || r.verdict === 'error').length })).sort((a, b) => a.score - b.score);
const summary = {
  generatedAt: new Date().toISOString(),
  engineVersion: JSON.parse(readFileSync(join(root, '.node/desktop/package.json'), 'utf8')).version,
  reference: `Chrome ${chromeVersion} (headless shell, Letter, no margin, backgrounds on)`,
  health,
  score,
  passRate,
  cases: results.length,
  pass: count('pass'),
  close: count('close'),
  fail: count('fail'),
  error: count('error'),
  thresholds: { pass: PASS, close: CLOSE, regression: REGRESSION },
  groups: Object.fromEntries(Object.entries(groups).map(([g, rs]) => [g, { cases: rs.length, score: +mean(rs.map((r) => r.score)).toFixed(2), pass: rs.filter((r) => r.verdict === 'pass').length }])),
  categories,
  regressions,
  pixelScore: +mean(results.filter((r) => r.pixel !== undefined).map((r) => r.pixel)).toFixed(2),
  worst: [...results].sort((a, b) => a.score - b.score).slice(0, 10).map(({ name, score, verdict, why }) => ({ name, score, verdict, why })),
};
writeFileSync(join(out, 'results.json'), JSON.stringify({ ...summary, results }, null, 2));
writeFileSync(join(here, 'latest.json'), JSON.stringify(summary, null, 2) + '\n');
// The demo page shows the summary, every category, and for each case the Chrome | pdf-engine | difference picture.
const demoDir = resolve(root, '.public/demo/parity');
rmSync(demoDir, { recursive: true, force: true });
mkdirSync(demoDir, { recursive: true });
for (const r of results) if (existsSync(join(out, 'png', `${r.name.replace('/', '__')}.png`))) copyFileSync(join(out, 'png', `${r.name.replace('/', '__')}.png`), resolve(root, '.public/demo', r.image));
writeFileSync(resolve(root, '.public/demo/parity.json'), JSON.stringify({ ...summary, results: results.map(({ name, category, score, pixel, verdict, why, image, pagesChrome, pagesEngine }) => ({ name, category, score, pixel, verdict, why, image, pagesChrome, pagesEngine })) }) + '\n');
if (flag('--update-baseline')) writeFileSync(baselinePath, JSON.stringify({ updatedAt: summary.generatedAt, score, cases: Object.fromEntries(results.map((r) => [r.name, r.score])) }, null, 2) + '\n');

const esc = (s) => String(s ?? '').replace(/[&<>]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;' })[c]);
writeFileSync(join(out, 'report.html'), `<!doctype html><meta charset=utf-8><title>pdf-engine parity</title>
<style>body{font:14px system-ui;margin:24px;max-width:1700px}h1{margin:0}.v-pass{color:#0a7d3b}.v-close{color:#b7791f}.v-fail,.v-error{color:#c53030}
table{border-collapse:collapse}td,th{padding:3px 10px;border-bottom:1px solid #ddd;text-align:left}img{max-width:100%;border:1px solid #ccc}details{margin:6px 0}</style>
<h1>Parity with Chrome: ${score} · ${health}</h1>
<p>${summary.cases} cases · ${summary.pass} pass · ${summary.close} close · ${summary.fail} fail · ${summary.error} error · reference ${esc(summary.reference)}.
Each image: Chrome | pdf-engine | difference (red only in Chrome, blue only in pdf-engine).</p>
${results.map((r) => `<details ${r.verdict === 'pass' ? '' : 'open'}><summary><b class="v-${r.verdict}">${r.verdict}</b> ${r.score} (pixel ${r.pixel ?? '-'}) · ${esc(r.name)} ${esc(r.why)}</summary><img loading=lazy src="png/${r.name.replace('/', '__')}.png"></details>`).join('\n')}`);

console.log(`\nparity ${score}  ${health}  (${summary.pass} pass, ${summary.close} close, ${summary.fail} fail, ${summary.error} error of ${summary.cases})`);
if (regressions.length) console.log('regressions:', regressions.map((x) => `${x.name} ${x.was} -> ${x.now}`).join(', '));
console.log(`report: ${join(out, 'report.html')}`);
if (flag('--check') && (health !== 'HEALTHY')) process.exit(1);
