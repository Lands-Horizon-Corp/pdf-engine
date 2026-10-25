// "No build step": a plain HTML page imports the package straight from a CDN-style URL.
// The package folder is served as a CDN such as jsDelivr serves it (files as they are, CORS open),
// and the page uses nothing but a <script type="module">.
//   node cdn.mjs            (needs `npm run build` in wasm/ and `npx playwright install` once)
import { chromium, firefox, webkit } from '@playwright/test';
import { createReadStream, existsSync, statSync } from 'node:fs';
import { createServer } from 'node:http';
import { extname, join, normalize, resolve } from 'node:path';

const pkg = resolve(import.meta.dirname, '..');
const types = { '.js': 'text/javascript', '.wasm': 'application/wasm', '.ttf': 'font/ttf', '.json': 'application/json' };
const page = `<!doctype html><meta charset="utf-8"><title>cdn</title>
<button id="go">go</button><pre id="out"></pre>
<script type="module">
  import { PDFEngine } from '/cdn/@lands-horizon-corp/pdf-engine-wasm@1/index.js';
  const pdf = new PDFEngine();
  document.querySelector('#go').addEventListener('click', async () => {
    const bytes = await pdf.generate({
      template: '<h1>Invoice {{ no }}</h1><ul>{% for i in items %}<li>{{ i.name }}: ₱{{ i.price }}</li>{% endfor %}</ul>',
      data: { no: '12345', items: [{ name: 'Hosting', price: 1500 }], unused: 'x'.repeat(1000) },
    });
    document.querySelector('#out').textContent = JSON.stringify({ head: String.fromCharCode(...bytes.slice(0, 5)), size: bytes.length });
  });
  window.ready = true;
</script>`;

const server = createServer((req, res) => {
  const path = decodeURIComponent(req.url.split('?')[0]);
  res.setHeader('Access-Control-Allow-Origin', '*');
  if (path === '/') return res.writeHead(200, { 'content-type': 'text/html' }).end(page);
  const rel = path.replace(/^\/cdn\/@lands-horizon-corp\/pdf-engine-wasm@[^/]+\//, '');
  const file = normalize(join(pkg, rel));
  if (rel === path || !file.startsWith(pkg) || !existsSync(file) || statSync(file).isDirectory()) return res.writeHead(404).end();
  res.writeHead(200, { 'content-type': types[extname(file)] || 'application/octet-stream' });
  createReadStream(file).pipe(res);
}).listen(0);
const url = `http://127.0.0.1:${server.address().port}/`;

let failed = false;
for (const [name, type] of [['chromium', chromium], ['webkit', webkit], ['firefox', firefox]]) {
  let browser;
  try {
    browser = await type.launch();
  } catch (e) {
    console.log(`${name}: could not start here (${String(e.message).split('\n')[0]})`);
    continue;
  }
  const errors = [];
  const tab = await browser.newPage();
  tab.on('pageerror', (e) => errors.push(e.message));
  await tab.goto(url);
  await tab.waitForFunction(() => window.ready);
  await tab.click('#go');
  await tab.waitForFunction(() => document.querySelector('#out').textContent, null, { timeout: 60_000 });
  const result = JSON.parse(await tab.textContent('#out'));
  const ok = result.head === '%PDF-' && result.size > 500 && errors.length === 0;
  console.log(`${name}: ${ok ? 'ok' : 'FAILED'} ${JSON.stringify(result)} ${errors.join('; ')}`);
  failed ||= !ok;
  await browser.close();
}
server.close();
process.exit(failed ? 1 : 0);
