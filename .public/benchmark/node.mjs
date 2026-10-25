// Per-document benchmark in Node: cold start, time per document and throughput, for each library.
//   PDF_ENGINE_BIN=/path/to/pdf-engine node node.mjs
// The 10,000-document run with memory is scale.mjs.
import { execSync } from 'node:child_process';
import { mkdirSync, statSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import { resolve } from 'node:path';
import { engineBinary, loadAll } from './libs.mjs';
import { scenarios } from './scenarios.js';
import { fmt, machine, median, p95, textChars } from './stats.js';

const CONCURRENCY = 8;
const now = () => performance.now();
const ids = ['pdf-engine', 'puppeteer-shell', 'puppeteer', 'html-pdf', 'wkhtmltopdf'];
const { ok: libraries, skipped } = await loadAll(ids);
for (const s of skipped) console.error(`${s.id} not measured: ${s.reason.slice(0, 100)}`);

async function measure(lib, scenario) {
  const { warm, burst } = lib.sizes;
  let i = 0;
  const doc = () => scenario.render(i++);
  // Cold start: nothing to the first PDF, five separate launches (median), so one slow first launch
  // (the OS checking a new binary) does not decide the result.
  const colds = [];
  for (let n = 0; n < 5; n++) {
    const started = now();
    const handle = await lib.make();
    await lib.render(handle, doc());
    colds.push(now() - started);
    await lib.close(handle);
  }

  const handle = await lib.make();
  const firstBytes = await lib.render(handle, doc());
  const times = [];
  for (let n = 0; n < warm; n++) {
    const d = doc();
    const t = now();
    await lib.render(handle, d);
    times.push(now() - t);
  }

  const t = now();
  let next = 0;
  await Promise.all(
    Array.from({ length: Math.min(CONCURRENCY, lib.concurrency) }, async (_, lane) => {
      const h = handle?.lane ? await handle.lane(lane) : handle;
      while (next < burst) {
        next++;
        await lib.render(h, doc());
      }
    }),
  );
  const perSecond = burst / ((now() - t) / 1000);
  const text = await textChars(firstBytes);
  await lib.close(handle);
  mkdirSync(resolve(import.meta.dirname, 'out'), { recursive: true });
  writeFileSync(resolve(import.meta.dirname, 'out', `node-${lib.id}-${scenario.name}.pdf`), firstBytes);
  return { library: lib.name, kind: lib.kind, coldMs: median(colds), medianMs: median(times), p95Ms: p95(times), perSecond, kb: firstBytes.length / 1024, chars: text.chars };
}

const out = [];
for (const docName of ['receipt', 'invoice']) {
  for (const lib of libraries) out.push({ doc: docName, ...(await measure(lib, scenarios[docName])) });
}

for (const doc of ['receipt', 'invoice']) {
  console.log(`\n### ${doc} (Node; throughput with up to ${CONCURRENCY} in flight)\n`);
  console.log('| Library | Kind | Cold start to first PDF (ms) | Per document, median (ms) | p95 (ms) | Documents/second | PDF size (KB) | Selectable text |');
  console.log('| --- | --- | ---: | ---: | ---: | ---: | ---: | --- |');
  for (const r of out.filter((x) => x.doc === doc)) {
    console.log(`| ${r.library} | ${r.kind} | ${fmt(r.coldMs)} | ${fmt(r.medianMs)} | ${fmt(r.p95Ms)} | ${Math.round(r.perSecond)} | ${r.kb.toFixed(0)} | ${r.chars > 0 ? 'yes' : 'no'} |`);
  }
}

const du = (path) => Number(execSync(`du -sk "${path}"`).toString().split('\t')[0]) / 1024;
const footprint = { 'pdf-engine': statSync(engineBinary()).size / 1048576 };
try {
  // The browsers Puppeteer downloaded (its cache folder; PUPPETEER_CACHE_DIR moves it).
  const cache = process.env.PUPPETEER_CACHE_DIR || resolve(os.homedir(), '.cache', 'puppeteer');
  footprint['Puppeteer (default)'] = du(resolve(cache, 'chrome'));
  footprint['Puppeteer (headless shell)'] = du(resolve(cache, 'chrome-headless-shell'));
} catch (e) {
  console.error(`Puppeteer install size not measured: ${e.message}`);
}
try {
  const { phantomPath, toolsDir } = await import('./tools.mjs');
  const phantom = phantomPath();
  if (phantom) footprint['html-pdf (PhantomJS)'] = du(resolve(phantom, '..'));
  footprint['wkhtmltopdf'] = du(resolve(toolsDir, 'wkhtmltox', 'bin')) + du(resolve(toolsDir, 'wkhtmltox', 'lib'));
} catch (e) {
  console.error(`PhantomJS and wkhtmltopdf install sizes not measured: ${e.message}`);
}
console.log(`\nInstall size (MB): ${Object.entries(footprint).map(([k, v]) => `${k} ${v.toFixed(0)}`).join(', ')}`);
mkdirSync(resolve(import.meta.dirname, 'results'), { recursive: true });
writeFileSync(resolve(import.meta.dirname, 'results', 'node.json'), JSON.stringify({ machine: machine(), footprint, rows: out }, null, 2));
