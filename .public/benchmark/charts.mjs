// Draws the README charts from the measured results (.public/benchmark/results/*.json).   node charts.mjs
// Plain SVG bar charts, one light and one dark version of each, written to .public/benchmark/charts/.
import { brotliCompressSync, constants } from 'node:zlib';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';

const here = import.meta.dirname;
const browser = JSON.parse(readFileSync(resolve(here, 'results/browser.json'), 'utf8'));
const node = JSON.parse(readFileSync(resolve(here, 'results/node.json'), 'utf8'));
const outDir = resolve(here, 'charts');
mkdirSync(outDir, { recursive: true });

const themes = {
  light: { bg: '#ffffff', border: '#d0d7de', text: '#1f2328', muted: '#656d76', ours: '#818cf8', best: '#1a7f37', other: '#8c959f', track: '#f3f4f6' },
  dark: { bg: '#0d1117', border: '#30363d', text: '#e6edf3', muted: '#8b949e', ours: '#7c83f5', best: '#3fb950', other: '#6e7681', track: '#161b22' },
};

const esc = (s) => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
const ms = (v) => (v < 10 ? `${v.toFixed(1)} ms` : `${Math.round(v)} ms`);
const perSecond = (v) => `${Math.round(v).toLocaleString('en-US')} / s`;
const duration = (sec) => (sec < 60 ? `${sec < 10 ? sec.toFixed(1) : Math.round(sec)} s` : sec < 3600 ? `${(sec / 60).toFixed(1)} min` : `${(sec / 3600).toFixed(1)} h`);
const megabytes = (v) => (v < 10 ? `${v.toFixed(1)} MB` : `${Math.round(v)} MB`);

/**
 * `groups`: [{ title, rows: [{ label, value, ours?, note? }] }]. The longest bar fills the width.
 */
function chart({ file, title, subtitle, format, groups, better = 'lower', badge = 'fastest', scope = 'chart' }) {
  const W = 880, pad = 24, labelW = 196, barX = pad + labelW + 8, barMax = 360, rowH = 30, groupGap = 16;
  const max = Math.max(...groups.flatMap((g) => g.rows.map((r) => r.value)));
  // The winner(s): the lowest value (or the highest for "longer is faster"), over the whole chart or per group.
  const scored = (r) => r.text === undefined;
  const bestOf = (rows) => {
    const values = rows.filter(scored).map((r) => r.value);
    return values.length ? (better === 'lower' ? Math.min(...values) : Math.max(...values)) : NaN;
  };
  const chartBest = bestOf(groups.flatMap((g) => g.rows));
  const isBest = (r, g) => scored(r) && r.value === (scope === 'group' ? bestOf(g.rows) : chartBest);
  const hasSoftOurs = groups.some((g) => g.rows.some((r) => r.ours && !isBest(r, g)));
  const heads = groups.filter((g) => g.title).length;
  const legendH = 22;
  const height = legendH + 74 + groups.reduce((h, g) => h + g.rows.length * rowH, 0) + heads * 22 + (groups.length - 1) * groupGap + 40;

  for (const [name, c] of Object.entries(themes)) {
    let y = 74;
    let body = '';
    for (const [gi, g] of groups.entries()) {
      if (gi > 0) y += groupGap;
      if (g.title) {
        body += `<text x="${pad}" y="${y + 4}" font-size="12" font-weight="600" fill="${c.muted}" letter-spacing="0.4">${esc(g.title.toUpperCase())}</text>`;
        y += 22;
      }
      for (const r of g.rows) {
        const w = Math.max(3, (r.value / max) * barMax);
        const cy = y + rowH / 2;
        const best = isBest(r, g);
        const fill = best ? c.best : r.ours ? c.ours : c.other;
        body +=
          `<text x="${pad}" y="${cy + 4.5}" font-size="14" fill="${c.text}" font-weight="${r.ours || best ? 700 : 400}">${esc(r.label)}</text>` +
          `<rect x="${barX}" y="${cy - 9}" width="${barMax}" height="18" rx="4" fill="${c.track}"/>` +
          `<rect x="${barX}" y="${cy - 9}" width="${w.toFixed(1)}" height="18" rx="4" fill="${fill}"/>` +
          `<text x="${(barX + w + 8).toFixed(1)}" y="${cy + 4.5}" font-size="14" fill="${c.text}" font-weight="${r.ours || best ? 700 : 500}">${esc(r.text ?? format(r.value))}${best ? ` <tspan fill="${c.best}" font-weight="700" font-size="12">\u2713 ${esc(badge)}</tspan>` : ''}${r.note ? ` <tspan fill="${c.muted}" font-weight="400" font-size="12">${esc(r.note)}</tspan>` : ''}</text>`;
        y += rowH;
      }
    }
    const svg =
      `<svg xmlns="http://www.w3.org/2000/svg" width="${W}" height="${height}" viewBox="0 0 ${W} ${height}" role="img" aria-labelledby="t d" font-family="-apple-system, BlinkMacSystemFont, 'Segoe UI', Helvetica, Arial, sans-serif">` +
      `<title id="t">${esc(title)}</title><desc id="d">${esc(subtitle)}</desc>` +
      `<rect x="0.5" y="0.5" width="${W - 1}" height="${height - 1}" rx="10" fill="${c.bg}" stroke="${c.border}"/>` +
      `<text x="${pad}" y="36" font-size="18" font-weight="700" fill="${c.text}">${esc(title)}</text>` +
      `<text x="${pad}" y="56" font-size="13" fill="${c.muted}">${esc(subtitle)}</text>` +
      body +
      legend(c, height, hasSoftOurs, badge) +
      `<text x="${pad}" y="${height - 14}" font-size="11" fill="${c.muted}">${esc(footer())}</text>` +
      `</svg>\n`;
    writeFileSync(resolve(outDir, `${file}-${name}.svg`), svg);
  }
}

/** The colour key above the footer: the winner, pdf-engine when it is not the winner, everyone else. */
function legend(c, height, showOurs, badge) {
  const items = [[c.best, badge], ...(showOurs ? [[c.ours, 'pdf-engine']] : []), [c.other, 'other libraries']];
  let x = 24;
  return items
    .map(([color, text]) => {
      const out = `<rect x="${x}" y="${height - 46}" width="10" height="10" rx="2" fill="${color}"/><text x="${x + 16}" y="${height - 37}" font-size="12" fill="${c.muted}">${esc(text)}</text>`;
      x += 16 + text.length * 6.6 + 18;
      return out;
    })
    .join('');
}

const footer = () => `${node.machine.cpu}, ${node.machine.platform}, measured ${node.machine.date}. Reproduce: .public/benchmark/README.md`;

// ---- data -------------------------------------------------------------------------------------
const bRow = (browserName, library) =>
  browser.rows.find((r) => r.browser.startsWith(browserName) && r.library === library && r.doc === 'invoice');
const nRow = (library) => node.rows.find((r) => r.library === library && r.doc === 'invoice');

const browserGroup = (name, title) => ({
  title,
  rows: browser.rows
    .filter((r) => r.browser.startsWith(name) && r.doc === 'invoice' && !r.unavailable)
    .map((r) => ({
      label: r.library === 'pdf-engine (WebAssembly)' ? 'pdf-engine' : r.library,
      value: r.medianMs,
      ours: r.library === 'pdf-engine (WebAssembly)',
    }))
    .sort((a, b) => a.value - b.value),
});

chart({
  file: 'browser-time',
  title: 'In the browser: time to make one invoice PDF',
  subtitle: 'Shorter is faster. 30-row invoice, median after warm-up. Built-in font = Latin only, no Unicode.',
  format: ms,
  scope: 'group',
  groups: [browserGroup('Chromium', 'Chrome, Edge, Brave (Chromium)'), browserGroup('WebKit', 'Safari (WebKit)')],
});

const htmlRows = (key) => [
  { label: 'pdf-engine', value: key(nRow('pdf-engine')), ours: true },
  { label: 'Puppeteer (headless shell)', value: key(nRow('Puppeteer (headless shell)')) },
  { label: 'Puppeteer (default)', value: key(nRow('Puppeteer (default)')) },
  { label: 'html-pdf', value: key(nRow('html-pdf (PhantomJS)')), note: 'PhantomJS, deprecated' },
  { label: 'wkhtmltopdf', value: key(nRow('wkhtmltopdf')), note: 'archived' },
].filter((r) => Number.isFinite(r.value));

chart({
  file: 'node-time',
  title: 'In Node: time to make one invoice PDF',
  subtitle: 'Shorter is faster. 30-row invoice, warm median.',
  format: ms,
  groups: [{ rows: htmlRows((r) => r.medianMs).sort((a, b) => a.value - b.value) }],
});

chart({
  file: 'node-throughput',
  title: 'In Node: invoices per second',
  subtitle: 'Longer is faster. Up to 8 requests in flight.',
  format: perSecond,
  better: 'higher',
  groups: [{ rows: htmlRows((r) => r.perSecond).sort((a, b) => b.value - a.value) }],
});

const wasm = resolve(here, '../../.node/wasm/dist/pdf_engine_wasm_bg.wasm');
const wasmMb = existsSync(wasm)
  ? brotliCompressSync(readFileSync(wasm), { params: { [constants.BROTLI_PARAM_QUALITY]: 9 } }).length / 1048576
  : null;
chart({
  file: 'install-size',
  title: 'Install size',
  subtitle: 'Smaller is lighter.',
  format: megabytes,
  badge: 'smallest',
  groups: [
    {
      rows: [
        ...(wasmMb ? [{ label: 'pdf-engine (in the browser)', value: wasmMb, ours: true, note: 'compressed' }] : []),
        { label: 'pdf-engine (native binary)', value: node.footprint['pdf-engine'], ours: true },
        { label: 'Puppeteer + headless shell', value: node.footprint['Puppeteer (headless shell)'] },
        { label: 'Puppeteer + Chrome', value: node.footprint['Puppeteer (default)'] },
        ...(node.footprint['html-pdf (PhantomJS)'] ? [{ label: 'html-pdf + PhantomJS', value: node.footprint['html-pdf (PhantomJS)'] }] : []),
        ...(node.footprint.wkhtmltopdf ? [{ label: 'wkhtmltopdf', value: node.footprint.wkhtmltopdf }] : []),
      ],
    },
  ],
});

// ---- the 10,000-document runs ------------------------------------------------------------------
const scalePath = resolve(here, 'results/scale.json');
if (existsSync(scalePath)) {
  const scale = JSON.parse(readFileSync(scalePath, 'utf8'));
  const label = {
    'pdf-engine': 'pdf-engine', 'puppeteer-shell': 'Puppeteer (headless shell)', puppeteer: 'Puppeteer (default)',
    wasm: 'pdf-engine (WebAssembly)', ironpress: 'ironpress (WebAssembly)', dompdf: 'dompdf.js', html2pdf: 'html2pdf.js', 'jspdf-html2canvas': 'jspdf-html2canvas',
    'html-pdf': 'html-pdf', wkhtmltopdf: 'wkhtmltopdf',
  };
  const note = { 'html-pdf': 'PhantomJS, x86 under Rosetta', wkhtmltopdf: 'x86 under Rosetta' };
  const groupsOf = [
    ['HTML to PDF (native / server)', ['pdf-engine', 'puppeteer-shell', 'puppeteer', 'html-pdf', 'wkhtmltopdf']],
    ['HTML to PDF in the browser', ['wasm', 'ironpress', 'dompdf', 'html2pdf', 'jspdf-html2canvas']],
  ];
  const titles = { small: 'receipts (1 page)', large: 'large invoices (300 lines, 7-9 pages)', multitable: 'multi-table statements (5 tables, 4-6 pages)' };
  const counts = scale.count.toLocaleString('en-US');

  for (const scenario of ['small', 'large', 'multitable']) {
    const rowsFor = (valueOf, fmt, projectedNote) =>
      groupsOf
        .map(([title, ids]) => ({
          title,
          rows: ids
            .map((id) => {
              const r = scale.rows.find((x) => x.id === id && x.scenario === scenario);
              if (!r) return null;
              const ours = id === 'pdf-engine' || id === 'wasm';
              if (!r.completed) return { label: label[id], value: 0, text: 'did not finish', note: r.firstError?.slice(0, 40), ours };
              const extra = [note[id], projectedNote && r.projected ? `projected from ${r.completed.toLocaleString('en-US')}` : null, r.errors ? `${r.errors} failed` : null].filter(Boolean).join(', ');
              return { label: label[id], value: valueOf(r), ours, note: extra || undefined };
            })
            .filter(Boolean)
            .sort((a, b) => a.value - b.value),
        }))
        .filter((g) => g.rows.length);

    const timeGroups = rowsFor((r) => (r.projected ? r.projectedSeconds : r.seconds), duration, true);
    chart({ file: `scale-time-${scenario}`, title: `${counts} ${titles[scenario]}: time to finish`, subtitle: 'Shorter is faster. Every document is different. "Projected" = did not finish in the time budget.', format: duration, groups: timeGroups });
  }
}


// ---- memory: one PDF of 1, 100 and 1000 pages -------------------------------------------------
const memoryPath = resolve(here, 'results/memory.json');
const memoryPages = { pages1: '1 page', pages100: '100 pages', pages1000: '1000 pages' };
if (existsSync(memoryPath)) {
  const memory = JSON.parse(readFileSync(memoryPath, 'utf8'));
  const groupsOf = [
    ['HTML to PDF (native / server)', ['pdf-engine', 'puppeteer-shell', 'puppeteer', 'html-pdf', 'wkhtmltopdf']],
    ['HTML to PDF in the browser (the whole browser counts)', ['wasm', 'ironpress', 'dompdf', 'html2pdf', 'jspdf-html2canvas']],
  ];
  for (const scenario of Object.keys(memoryPages)) {
    const groups = groupsOf
      .map(([title, ids]) => ({
        title,
        rows: ids
          .map((id) => {
            const r = memory.rows.find((x) => x.id === id && x.scenario === scenario);
            if (!r) return null;
            const ours = id === 'pdf-engine' || id === 'wasm';
            if (r.status !== 'ok') return { label: r.library || id, value: 0, text: 'failed', note: String(r.status).slice(0, 40), ours };
            return { label: r.library, value: r.peakMb, ours };
          })
          .filter(Boolean)
          .sort((a, b) => (a.value || Infinity) - (b.value || Infinity)),
      }))
      .filter((g) => g.rows.length);
    if (groups.length) chart({ file: `memory-${scenario}`, title: `One PDF of ${memoryPages[scenario]}: peak memory`, subtitle: 'Shorter is better. Peak resident memory of the process and everything it starts, in a fresh process.', format: megabytes, groups, badge: 'least memory' });
  }
}

// ---- the README block: charts and tables, regenerated from the results ------------------------
const readmePath = resolve(here, '../../README.md');
if (existsSync(readmePath) && readFileSync(readmePath, 'utf8').includes('<!-- bench:start -->')) {
  const picture = (file, alt) =>
    `<picture>\n  <source media="(prefers-color-scheme: dark)" srcset=".public/benchmark/charts/${file}-dark.svg">\n  <img alt="${alt}" src=".public/benchmark/charts/${file}-light.svg">\n</picture>\n`;
  const table = (head, rows) => `| ${head.join(' | ')} |\n| ${head.map((h, i) => (i === 0 ? '---' : '---:')).join(' | ')} |\n${rows.map((r) => `| ${r.join(' | ')} |`).join('\n')}\n`;
  let md = '';

  if (existsSync(scalePath)) {
    const scale = JSON.parse(readFileSync(scalePath, 'utf8'));
    const counts = scale.count.toLocaleString('en-US');
    const names = { small: 'Receipts', large: 'Large invoices', multitable: 'Multi-table statements' };
    const about = { small: '8 lines, 1 page', large: '300 lines, 7-9 pages', multitable: '5 tables of 35 lines, 4-6 pages' };
    const display = (r) => `${r.library}${r.id.endsWith('-builtin') ? '' : ''}`;
    md += `### ${counts} documents, every library\n\nEach library makes ${counts} different documents (or runs for ${scale.budgetSeconds} s, then the time is projected). Time to finish, three kinds of document:\n\n`;
    for (const scenario of ['small', 'large', 'multitable']) {
      md += `#### ${names[scenario]} (${about[scenario]})\n\n${picture(`scale-time-${scenario}`, `${names[scenario]}: time to make ${counts} documents`)}\n`;
    }
    md += '<details>\n<summary>All the numbers</summary>\n\n';
    for (const scenario of ['small', 'large', 'multitable']) {
      const rows = scale.rows.filter((r) => r.scenario === scenario).sort((a, b) => (a.projectedSeconds ?? a.seconds) - (b.projectedSeconds ?? b.seconds));
      md += `**${names[scenario]}** (${about[scenario]})\n\n` + table(
        ['Library', 'Finished', `Time for ${counts}`, 'Documents / second', 'Pages'],
        rows.map((r) => [
          r.library,
          r.errors ? `${r.completed.toLocaleString('en-US')} (${r.errors} failed)` : r.completed.toLocaleString('en-US'),
          r.completed ? `${duration(r.projected ? r.projectedSeconds : r.seconds)}${r.projected ? ' (projected)' : ''}` : 'did not finish',
          r.completed ? Math.round(r.perSecond).toLocaleString('en-US') : 'n/a',
          r.pages ?? 'n/a',
        ]),
      ) + '\n';
    }
    const skipped = (scale.skipped || []).map((x) => `${x.id}: ${x.reason}`).join('; ');
    if (skipped) md += `Not run here: ${skipped}\n\n`;
    md += `Machine: ${scale.machine.cpu}, ${scale.machine.cores} cores, ${scale.machine.platform}, Node ${scale.machine.node}, ${scale.machine.date}.\n\n</details>\n\n`;
  }

  if (existsSync(memoryPath)) {
    const memory = JSON.parse(readFileSync(memoryPath, 'utf8'));
    md += `### Memory: one PDF of 1, 100 and 1000 pages\n\nEach library makes **one** PDF in a fresh process, cold (nothing warmed up). The number is the peak resident memory of that process and everything it starts (Chromium, PhantomJS, wkhtmltopdf), sampled every 20 ms from the start until the PDF is made, so it includes the cost of starting. The Node libraries are run from a Node process, which is about 55 MB of every Node figure here; the browser libraries run in Chromium. A library that cannot make the document is shown as failed.\n\n`;
    for (const scenario of Object.keys(memoryPages)) md += `${picture(`memory-${scenario}`, `Peak memory for one PDF of ${memoryPages[scenario]}`)}\n`;
    const ids = [...new Set(memory.rows.map((r) => r.id))];
    const cell = (id, sc) => {
      const r = memory.rows.find((x) => x.id === id && x.scenario === sc);
      if (!r) return 'n/a';
      return r.status === 'ok' ? `${Math.round(r.peakMb).toLocaleString('en-US')} MB, ${duration(r.seconds)}` : `failed (${Math.round(r.peakMb).toLocaleString('en-US')} MB peak)`;
    };
    const wanted = { pages1: 1, pages100: 100, pages1000: 1000 };
    const notes = [];
    for (const sc of Object.keys(memoryPages)) {
      const ok = memory.rows.filter((r) => r.scenario === sc && r.status === 'ok');
      const least = [...ok].sort((a, b) => a.peakMb - b.peakMb)[0];
      const ours = ok.find((r) => r.id === 'pdf-engine');
      if (least && ours && least.id !== 'pdf-engine') notes.push(`for ${memoryPages[sc]}, ${least.library} used the least (${Math.round(least.peakMb)} MB against ${Math.round(ours.peakMb)} MB for pdf-engine)`);
      for (const r of ok) if (r.pages && r.pages !== wanted[sc] && r.pages !== wanted[sc] + 1) notes.push(`${r.library} made ${r.pages} pages for ${memoryPages[sc]}`);
      for (const r of memory.rows.filter((x) => x.scenario === sc && x.status !== 'ok')) notes.push(`${r.library} could not make ${memoryPages[sc]} (${String(r.status).slice(0, 80)})`);
    }
    if (notes.length) md += `Read it with these in mind: ${notes.join('; ')}. wkhtmltopdf and html-pdf are x86 builds run under Rosetta on this Mac, and they hold a document differently (they make one blank page more than asked). pdf-engine builds the whole document in memory before writing it (about 0.75 MB per page here), which is why it is behind the older engines from 100 pages on; it is the lowest at 1 page. One run each.\n\n`;
    md += `**Why the numbers look like this**

- **pdf-engine** lays out the whole document in memory before it writes the first page. What it keeps, per page of this test
  (30 table rows, 90 cells): about 215 nodes of 1.2 KB each, the text layout of each cell (about 3 KB each, from the text
  shaping library), and the styles of the elements. That is about 0.75 MB per page, so memory grows with the page count: 22 MB
  idle, 89 MB for 1 page, 162 MB for 100, 803 MB for 1000. Before 1.4.1 it also read every installed font at start (about
  200 MB on a Mac) and kept a second copy of the style of every table cell, which is why 1.4.0 was higher.
- **wkhtmltopdf and html-pdf** (WebKit, in C++) hold the same document in compact structures and write the PDF as they go, so they
  start higher than an idle pdf-engine but grow much more slowly (about 0.2 MB per page here). This is our reading of the
  numbers; we have not profiled their insides.
- **Puppeteer** runs a whole browser (several processes), which is most of its 1 to 3 GB, and the browser libraries (pdf-engine's
  WebAssembly, ironpress, dompdf.js, html2pdf.js, jspdf-html2canvas) are measured with the Chromium that runs them, about 550 MB
  before they do anything. html2pdf.js and jspdf-html2canvas also draw each page as an image first.
- **The fix is not a tweak:** to use less than the older engines for long documents, pdf-engine would have to lay out and write
  the document a few pages at a time instead of all at once. That is planned work, not part of 1.4.1.

`;
    md += '<details>\n<summary>All the numbers (peak memory, time to make the PDF)</summary>\n\n' + table(['Library', ...Object.values(memoryPages)], ids.map((id) => [memory.rows.find((x) => x.id === id).library || id, ...Object.keys(memoryPages).map((sc) => cell(id, sc))])) + `\nMachine: ${memory.machine.cpu}, ${memory.machine.cores} cores, ${memory.machine.platform}, Node ${memory.machine.node}, ${memory.machine.date}. One run each, so treat small differences as noise.\n\n</details>\n\n`;
  }

  md += `### Speed per document\n\n${picture('node-time', 'Node: time per invoice')}\n${picture('node-throughput', 'Node: invoices per second')}\n${picture('browser-time', 'Browser: time per invoice')}\n${picture('install-size', 'Install size')}\n`;

  const current = readFileSync(readmePath, 'utf8');
  const next = current.replace(/<!-- bench:start -->[\s\S]*?<!-- bench:end -->/, () => `<!-- bench:start -->\n<!-- Generated by .public/benchmark/charts.mjs (npm run charts). Do not edit by hand. -->\n\n${md}<!-- bench:end -->`);
  if (next !== current) writeFileSync(readmePath, next);
}

console.log(`charts written to ${outDir}`);
