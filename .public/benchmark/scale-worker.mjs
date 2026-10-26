// Runs ONE library on ONE scenario for `count` documents (or until the time budget), in this process,
// and prints a line "RESULT {json}". scale.mjs starts it and measures its memory.
//   node scale-worker.mjs <library id> <scenario> <count> <budget seconds>
import { createReadStream, existsSync, statSync } from 'node:fs';
import { createServer } from 'node:http';
import { extname, join, normalize } from 'node:path';
import { loadDriver } from './libs.mjs';
import { scenarios } from './scenarios.js';
import { textChars } from './stats.js';

const [id, scenarioName, countArg, budgetArg] = process.argv.slice(2);
const count = Number(countArg);
const budgetMs = Number(budgetArg) * 1000;
const scenario = scenarios[scenarioName];

/** Libraries that only exist in a browser run in Chromium through Playwright. */
const browserLibraries = { wasm: 'pdf-engine (WebAssembly)', ironpress: 'ironpress', dompdf: 'dompdf.js', html2pdf: 'html2pdf.js', 'jspdf-html2canvas': 'jspdf-html2canvas' };

async function inBrowser() {
  const { chromium } = await import('playwright');
  const dist = join(import.meta.dirname, 'browser', 'dist');
  const types = { '.html': 'text/html', '.js': 'text/javascript', '.wasm': 'application/wasm', '.css': 'text/css', '.ttf': 'font/ttf' };
  const server = createServer((req, res) => {
    const file = join(dist, normalize(decodeURIComponent(req.url.split('?')[0])).replace(/^\/$/, '/index.html'));
    if (!file.startsWith(dist) || !existsSync(file) || statSync(file).isDirectory()) return res.writeHead(404).end();
    res.writeHead(200, { 'content-type': types[extname(file)] || 'application/octet-stream' });
    createReadStream(file).pipe(res);
  }).listen(0);
  const browser = await chromium.launch();
  const page = await browser.newPage();
  await page.goto(`http://127.0.0.1:${server.address().port}/`);
  await page.waitForFunction(() => window.benchReady);
  const r = await page.evaluate(([l, s, n, b]) => window.bench.batch(l, s, n, b), [browserLibraries[id], scenarioName, count, budgetMs]);
  await browser.close();
  server.close();
  return { name: browserLibraries[id], kind: ['html2pdf', 'jspdf-html2canvas'].includes(id) ? 'HTML to PDF (a picture)' : 'HTML to PDF', seconds: r.ms / 1000, completed: r.completed, errors: r.errors, firstError: r.firstError, first: r.first ? Buffer.from(r.first, 'base64') : null };
}

async function inNode() {
  const driver = await loadDriver(id);
  const handle = await driver.make();
  for (let i = 0; i < 3; i++) await driver.render(handle, scenario.render(i)); // warm-up, not counted
  let next = 0;
  let completed = 0;
  let errors = 0;
  let firstError = null;
  let first = null;
  const started = performance.now();
  await Promise.all(
    Array.from({ length: driver.concurrency }, async (_, lane) => {
      const h = handle?.lane ? await handle.lane(lane) : handle;
      for (;;) {
        const i = next++;
        if (i >= count || performance.now() - started > budgetMs) return;
        try {
          const bytes = await driver.render(h, scenario.render(i));
          if (i === 0) first = bytes;
          completed++;
        } catch (e) {
          errors++;
          firstError ??= String(e?.message || e).split('\n')[0];
        }
      }
    }),
  );
  const seconds = (performance.now() - started) / 1000;
  await driver.close(handle);
  return { name: driver.name, kind: driver.kind, seconds, completed, errors, firstError, first };
}

const r = browserLibraries[id] ? await inBrowser() : await inNode();
let pages = null;
let chars = null;
let kb = null;
if (r.first) {
  const t = await textChars(r.first);
  [pages, chars, kb] = [t.pages, t.chars, r.first.length / 1024];
}
console.log(`RESULT ${JSON.stringify({ id, library: r.name, kind: r.kind, scenario: scenarioName, count, completed: r.completed, errors: r.errors, firstError: r.firstError, seconds: r.seconds, pages, chars, kb })}`);
process.exit(0);
