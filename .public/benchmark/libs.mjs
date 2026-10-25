// The HTML-to-PDF libraries that run in Node, one driver each:  { id, name, kind, concurrency, make, render, close }
//   make()              -> a handle (an engine, a browser, ...)
//   render(handle, doc) -> Uint8Array   (doc = scenario.render(i): { html })
//   close(handle)
// The ones that only exist in a browser (pdf-engine's WebAssembly, html2pdf.js, jspdf-html2canvas) are in browser/src/page.js.
import { spawn } from 'node:child_process';
import { createRequire } from 'node:module';
import { resolve } from 'node:path';
import { wkhtmltopdfPath } from './tools.mjs';

const require = createRequire(import.meta.url);
const here = import.meta.dirname;

/** The engine binary: PDF_ENGINE_BIN, else the release build. */
export const engineBinary = () => process.env.PDF_ENGINE_BIN || resolve(here, '../../target/release/pdf-engine');

// ---- the drivers ------------------------------------------------------------------------------
const drivers = {};

drivers['pdf-engine'] = async () => {
  const { PDFEngine } = await import('@lands-horizon-corp/pdf-engine');
  return {
    name: 'pdf-engine',
    kind: 'HTML to PDF',
    concurrency: 8,
    make: async () => new PDFEngine({ binary: engineBinary() }),
    render: (pdf, doc) => pdf.generate({ template: doc.html }),
    close: (pdf) => pdf.close(),
    sizes: { warm: 100, burst: 400 },
  };
};

for (const [id, name, headless] of [
  ['puppeteer', 'Puppeteer (default)', true],
  ['puppeteer-shell', 'Puppeteer (headless shell)', 'shell'],
]) {
  drivers[id] = async () => {
    const puppeteer = (await import('puppeteer')).default;
    return {
      name,
      kind: 'HTML to PDF',
      concurrency: 8,
      make: async () => {
        const browser = await puppeteer.launch({ headless });
        const pages = new Map();
        const lane = async (i) => pages.get(i) || pages.set(i, await browser.newPage()).get(i);
        return { browser, lane, page: await lane('main') };
      },
      render: async (h, doc) => {
        const page = h.lane ? h.page : h;
        await page.setContent(doc.html, { waitUntil: 'load' });
        return new Uint8Array(await page.pdf({ format: 'Letter', printBackground: true }));
      },
      close: (h) => h.browser.close(),
      sizes: { warm: 25, burst: 64 },
    };
  };
}

// html-pdf drives PhantomJS (discontinued in 2018; `html-pdf` is deprecated on npm). One PhantomJS process per document.
drivers['html-pdf'] = async () => {
  const htmlPdf = require('html-pdf');
  const probe = await new Promise((ok) => htmlPdf.create('<p>x</p>').toBuffer((e, b) => ok(e ? String(e.message || e) : b)));
  if (typeof probe === 'string') throw new Error(probe);
  return {
    name: 'html-pdf (PhantomJS)',
    kind: 'HTML to PDF',
    concurrency: 8,
    make: async () => htmlPdf,
    render: (lib, doc) => new Promise((ok, fail) => lib.create(doc.html, { format: 'Letter', border: '0' }).toBuffer((e, b) => (e ? fail(e) : ok(new Uint8Array(b))))),
    close: () => {},
    sizes: { warm: 10, burst: 24 },
  };
};

// wkhtmltopdf is a program, not a library: one process per document, HTML in on stdin, PDF out on stdout. The npm
// package of that name only starts the same program. Smart shrinking is off and the zoom is 1.3 so a page holds what
// it holds in Chrome (by default it shrinks the content to about 0.77 and makes fewer pages).
drivers['wkhtmltopdf'] = async () => {
  const program = wkhtmltopdfPath();
  const render = (doc) =>
    new Promise((ok, fail) => {
      const child = spawn(program, ['-q', '--page-size', 'Letter', '-T', '0', '-R', '0', '-B', '0', '-L', '0', '--disable-smart-shrinking', '--zoom', '1.3', '-', '-'], { stdio: ['pipe', 'pipe', 'pipe'] });
      const chunks = [];
      let err = '';
      child.stdout.on('data', (c) => chunks.push(c));
      child.stderr.on('data', (c) => (err += c));
      child.on('error', fail);
      child.on('close', (code) => (chunks.length ? ok(new Uint8Array(Buffer.concat(chunks))) : fail(new Error(`wkhtmltopdf exited with ${code}: ${err.trim().split('\n').pop()}`))));
      child.stdin.end(doc.html);
    });
  await render({ html: '<p>x</p>' }); // fails here, with the reason, when the program is missing
  return {
    name: 'wkhtmltopdf',
    kind: 'HTML to PDF',
    concurrency: 8,
    make: async () => null,
    render: (_, doc) => render(doc),
    close: () => {},
    sizes: { warm: 10, burst: 24 },
  };
};

export const driverIds = Object.keys(drivers);

/** The driver for `id`, or throws with the reason it cannot run here. */
export async function loadDriver(id) {
  if (!drivers[id]) throw new Error(`unknown library "${id}"`);
  return { id, ...(await drivers[id]()) };
}

/** Every driver that runs on this machine, and the reasons the others do not. */
export async function loadAll(ids = driverIds) {
  const ok = [];
  const skipped = [];
  for (const id of ids) {
    try {
      ok.push(await loadDriver(id));
    } catch (e) {
      skipped.push({ id, reason: String(e.message).split('\n')[0] });
    }
  }
  return { ok, skipped };
}
