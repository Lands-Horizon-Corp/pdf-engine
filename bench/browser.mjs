// Browser benchmark: pdf-engine (WebAssembly) vs html2pdf.js vs jspdf-html2canvas, in each browser.
//   npm run build && node browser.mjs
import { chromium, firefox, webkit } from 'playwright';
import { createReadStream, existsSync, mkdirSync, statSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { extname, join, normalize } from 'node:path';
import { fmt, machine, median, p95, textChars } from './stats.js';

const dist = join(import.meta.dirname, 'browser', 'dist');
const outDir = join(import.meta.dirname, 'out');
mkdirSync(outDir, { recursive: true });
const types = { '.html': 'text/html', '.js': 'text/javascript', '.wasm': 'application/wasm', '.css': 'text/css', '.ttf': 'font/ttf' };
const server = createServer((req, res) => {
  const file = join(dist, normalize(decodeURIComponent(req.url.split('?')[0])).replace(/^\/$/, '/index.html'));
  if (!file.startsWith(dist) || !existsSync(file) || statSync(file).isDirectory()) return res.writeHead(404).end();
  res.writeHead(200, { 'content-type': types[extname(file)] || 'application/octet-stream' });
  createReadStream(file).pipe(res);
}).listen(0);
const url = `http://127.0.0.1:${server.address().port}/`;

// The libraries that draw pages from code and pdf-engine take milliseconds, so they get many runs; the
// two that photograph the page take a second or more.
const SLOW = new Set(['html2pdf.js', 'jspdf-html2canvas']);
const runsFor = (library) => (SLOW.has(library) ? 8 : 100);
const warmFor = (library) => (SLOW.has(library) ? 0 : 300);
const results = [];

for (const [name, type] of [['Chromium (Chrome, Edge, Brave)', chromium], ['WebKit (Safari)', webkit], ['Firefox', firefox]]) {
  let browser;
  try {
    browser = await type.launch();
  } catch (e) {
    console.error(`${name}: could not start here (${String(e.message).split('\n')[0]})`);
    results.push({ browser: name, unavailable: true });
    continue;
  }
  for (const docName of ['receipt', 'invoice']) {
    const libraryNames = await (async () => {
      const probe = await browser.newPage();
      await probe.goto(url);
      await probe.waitForFunction(() => window.benchReady);
      const names = await probe.evaluate(() => window.bench.libraries);
      await probe.close();
      return names;
    })();
    for (const library of libraryNames) {
      // A fresh page per measurement, so "first call" really is the first call (module loading included).
      const page = await browser.newPage();
      await page.goto(url);
      await page.waitForFunction(() => window.benchReady);
      const r = await page.evaluate(([l, d, n, w]) => window.bench.run(l, d, n, w), [library, docName, runsFor(library), warmFor(library)]);
      await page.close();
      const bytes = Buffer.from(r.bytes, 'base64');
      writeFileSync(join(outDir, `${type.name()}-${docName}-${library.replace(/[^a-z0-9]+/gi, '_')}.pdf`), bytes);
      const text = await textChars(bytes);
      results.push({ browser: name, doc: docName, library, firstMs: r.first, medianMs: median(r.times), p95Ms: p95(r.times), kb: bytes.length / 1024, chars: text.chars, pages: text.pages });
    }
  }
  await browser.close();
}
server.close();
mkdirSync(join(import.meta.dirname, 'results'), { recursive: true });
writeFileSync(join(import.meta.dirname, 'results', 'browser.json'), JSON.stringify({ machine: machine(), rows: results }, null, 2));

for (const doc of ['receipt', 'invoice']) {
  console.log(`\n### ${doc}\n`);
  console.log('| Browser | Library | First call (ms) | Per document, median (ms) | p95 (ms) | PDF size (KB) | Selectable text |');
  console.log('| --- | --- | ---: | ---: | ---: | ---: | --- |');
  for (const r of results.filter((x) => x.doc === doc)) {
    console.log(`| ${r.browser} | ${r.library} | ${fmt(r.firstMs)} | ${fmt(r.medianMs)} | ${fmt(r.p95Ms)} | ${r.kb.toFixed(0)} | ${r.chars > 0 ? `yes (${r.chars} chars)` : 'no (image)'} |`);
  }
}
for (const r of results.filter((x) => x.unavailable)) console.log(`\n${r.browser}: not measured (could not start on this machine)`);
