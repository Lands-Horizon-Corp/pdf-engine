// The libraries under test, one driver each:  { id, name, kind, make, render, close }
//   make()              -> a handle (an engine, a browser, ...)
//   render(handle, doc) -> Uint8Array   (doc = scenario.render(i): { html, data })
//   close(handle)
//
// Libraries that draw from code (pdfkit, pdf-lib, jsPDF) share one page layout, so they draw exactly
// the same page and only the library differs. pdfmake and @react-pdf/renderer lay out the page
// themselves, from the same data.
import { createRequire } from 'node:module';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

const require = createRequire(import.meta.url);
const here = import.meta.dirname;
const FONT = resolve(here, '../assets/fonts/NotoSans-Regular.ttf');
const FONT_BOLD = resolve(here, '../assets/fonts/NotoSans-Bold.ttf');
const FONT_BUF = readFileSync(FONT);
const FONT_BOLD_BUF = readFileSync(FONT_BOLD);

/** The engine binary: PDF_ENGINE_BIN, else the release build. */
export const engineBinary = () => process.env.PDF_ENGINE_BIN || resolve(here, '../../target/release/pdf-engine');

import { PAGE, layout } from './layout.js';
export { layout, PAGE };

const hexToRgb = (hex) => [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255);

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

// html-pdf drives PhantomJS, which is discontinued and has no Apple Silicon build.
drivers['html-pdf'] = async () => {
  const htmlPdf = require('html-pdf');
  const probe = await new Promise((ok) => htmlPdf.create('<p>x</p>').toBuffer((e, b) => ok(e ? String(e.message || e) : b)));
  if (typeof probe === 'string') throw new Error(probe);
  return {
    name: 'html-pdf (PhantomJS)',
    kind: 'HTML to PDF',
    concurrency: 4,
    make: async () => htmlPdf,
    render: (lib, doc) => new Promise((ok, fail) => lib.create(doc.html, { format: 'Letter' }).toBuffer((e, b) => (e ? fail(e) : ok(new Uint8Array(b))))),
    close: () => {},
    sizes: { warm: 10, burst: 24 },
  };
};

// pdfkit ----------------------------------------------------------------------------------------
for (const [id, name, unicode] of [
  ['pdfkit', 'pdfkit (Noto Sans)', true],
  ['pdfkit-builtin', 'pdfkit (built-in font)', false],
]) {
  drivers[id] = async () => {
    const PDFDocument = require('pdfkit');
    const [regular, bold, peso] = unicode ? ['Noto', 'NotoBold', '₱'] : ['Helvetica', 'Helvetica-Bold', 'PHP '];
    return {
      name,
      kind: 'PDF from code',
      concurrency: 1,
      make: async () => PDFDocument,
      render: (Doc, { data }) =>
        new Promise((ok, fail) => {
          const doc = new Doc({ size: 'LETTER', margin: 0, autoFirstPage: false });
          if (unicode) doc.registerFont('Noto', FONT_BUF).registerFont('NotoBold', FONT_BOLD_BUF);
          const chunks = [];
          doc.on('data', (c) => chunks.push(c)).on('end', () => ok(new Uint8Array(Buffer.concat(chunks)))).on('error', fail);
          for (const ops of layout(data, peso)) {
            doc.addPage({ size: 'LETTER', margin: 0 });
            for (const op of ops) {
              if (op.k === 'text') doc.font(op.bold ? bold : regular).fontSize(op.size).fillColor(op.color).text(op.t, op.x, op.y, { width: op.w, align: op.align, lineBreak: false });
              else if (op.k === 'rect') doc.rect(op.x, op.y, op.w, op.h).fill(op.color);
              else doc.moveTo(op.x1, op.y).lineTo(op.x2, op.y).lineWidth(0.5).strokeColor(op.color).stroke();
            }
          }
          doc.end();
        }),
      close: () => {},
      sizes: { warm: 50, burst: 200 },
    };
  };
}

// pdf-lib ---------------------------------------------------------------------------------------
for (const [id, name, unicode] of [
  ['pdf-lib', 'pdf-lib (Noto Sans)', true],
  ['pdf-lib-builtin', 'pdf-lib (built-in font)', false],
]) {
  drivers[id] = async () => {
    const { PDFDocument, StandardFonts, rgb } = require('pdf-lib');
    const fontkit = require('@pdf-lib/fontkit');
    const peso = unicode ? '₱' : 'PHP ';
    return {
      name,
      kind: 'PDF from code',
      concurrency: 1,
      make: async () => null,
      render: async (_, { data }) => {
        const pdf = await PDFDocument.create();
        let regular;
        let bold;
        if (unicode) {
          pdf.registerFontkit(fontkit);
          regular = await pdf.embedFont(FONT_BUF, { subset: true });
          bold = await pdf.embedFont(FONT_BOLD_BUF, { subset: true });
        } else {
          regular = await pdf.embedFont(StandardFonts.Helvetica);
          bold = await pdf.embedFont(StandardFonts.HelveticaBold);
        }
        for (const ops of layout(data, peso)) {
          const page = pdf.addPage([PAGE.w, PAGE.h]);
          for (const op of ops) {
            const color = rgb(...hexToRgb(op.color));
            if (op.k === 'text') {
              const font = op.bold ? bold : regular;
              const width = op.align === 'right' ? font.widthOfTextAtSize(op.t, op.size) : 0;
              page.drawText(op.t, { x: op.align === 'right' ? op.x + op.w - width : op.x, y: PAGE.h - op.y - op.size, size: op.size, font, color });
            } else if (op.k === 'rect') page.drawRectangle({ x: op.x, y: PAGE.h - op.y - op.h, width: op.w, height: op.h, color });
            else page.drawLine({ start: { x: op.x1, y: PAGE.h - op.y }, end: { x: op.x2, y: PAGE.h - op.y }, thickness: 0.5, color });
          }
        }
        return pdf.save();
      },
      close: () => {},
      sizes: { warm: 50, burst: 200 },
    };
  };
}

// jsPDF -----------------------------------------------------------------------------------------
for (const [id, name, unicode] of [
  ['jspdf', 'jsPDF (Noto Sans)', true],
  ['jspdf-builtin', 'jsPDF (built-in font)', false],
]) {
  drivers[id] = async () => {
    const { jsPDF } = require('jspdf');
    const peso = unicode ? '₱' : 'PHP ';
    const regular64 = FONT_BUF.toString('base64');
    const bold64 = FONT_BOLD_BUF.toString('base64');
    return {
      name,
      kind: 'PDF from code',
      concurrency: 1,
      make: async () => null,
      render: async (_, { data }) => {
        const doc = new jsPDF({ unit: 'pt', format: 'letter', compress: true });
        const family = unicode ? 'Noto' : 'helvetica';
        if (unicode) {
          doc.addFileToVFS('Noto-Regular.ttf', regular64).addFont('Noto-Regular.ttf', 'Noto', 'normal');
          doc.addFileToVFS('Noto-Bold.ttf', bold64).addFont('Noto-Bold.ttf', 'Noto', 'bold');
        }
        layout(data, peso).forEach((ops, index) => {
          if (index > 0) doc.addPage();
          for (const op of ops) {
            if (op.k === 'text') {
              doc.setFont(family, op.bold ? 'bold' : 'normal').setFontSize(op.size).setTextColor(op.color);
              doc.text(op.t, op.align === 'right' ? op.x + op.w : op.x, op.y + op.size * 0.85, { align: op.align });
            } else if (op.k === 'rect') doc.setFillColor(op.color).rect(op.x, op.y, op.w, op.h, 'F');
            else doc.setDrawColor(op.color).setLineWidth(0.5).line(op.x1, op.y, op.x2, op.y);
          }
        });
        return new Uint8Array(doc.output('arraybuffer'));
      },
      close: () => {},
      sizes: { warm: 50, burst: 200 },
    };
  };
}

// pdfmake ---------------------------------------------------------------------------------------
for (const [id, name, unicode] of [
  ['pdfmake', 'pdfmake (Noto Sans)', true],
  ['pdfmake-builtin', 'pdfmake (built-in font)', false],
]) {
  drivers[id] = async () => {
    const pdfmake = require('pdfmake');
    // pdfmake reads fonts by name from its in-memory file system, so they are not read from disk per document.
    pdfmake.virtualfs.writeFileSync('Noto-Regular.ttf', FONT_BUF);
    pdfmake.virtualfs.writeFileSync('Noto-Bold.ttf', FONT_BOLD_BUF);
    pdfmake.setFonts({
      Noto: { normal: 'Noto-Regular.ttf', bold: 'Noto-Bold.ttf' },
      Helvetica: { normal: 'Helvetica', bold: 'Helvetica-Bold', italics: 'Helvetica-Oblique', bolditalics: 'Helvetica-BoldOblique' },
    });
    const [font, peso] = unicode ? ['Noto', '₱'] : ['Helvetica', 'PHP '];
    const right = (text, extra = {}) => ({ text: String(text), alignment: 'right', ...extra });
    const definition = ({ data }) => ({
      pageSize: 'LETTER',
      pageMargins: [32, 32, 32, 32],
      defaultStyle: { font, fontSize: 9 },
      content: [
        { columns: [{ text: 'Lands Horizon\n123 Ayala Ave, Makati', fontSize: 16, bold: true, color: '#4f46e5' }, { text: `${data.title}\n#${data.no}\n24 Oct 2026`, alignment: 'right', bold: true }] },
        { text: 'Billed to Acme Corp', margin: [0, 10, 0, 6] },
        ...data.sections.flatMap((s) => [
          ...(s.heading ? [{ text: s.heading, bold: true, fontSize: 12, color: '#3730a3', margin: [0, 10, 0, 4] }] : []),
          {
            table: {
              headerRows: 1,
              dontBreakRows: true,
              widths: ['*', 40, 60, 70],
              body: [
                ['Description', 'Qty', 'Unit', 'Amount'].map((text) => ({ text, bold: true, fillColor: '#eef2ff' })),
                ...s.rows.map((r) => [r.name, right(r.qty), right(`${peso}${r.price}`), right(`${peso}${r.qty * r.price}`)]),
                ...(data.sections.length > 1 ? [[{ text: 'Subtotal', bold: true, colSpan: 3 }, {}, {}, right(`${peso}${s.subtotal}`, { bold: true })]] : []),
              ],
            },
            layout: 'lightHorizontalLines',
          },
        ]),
        { text: `Total ${peso}${data.total}`, alignment: 'right', bold: true, fontSize: 12, margin: [0, 10, 0, 0] },
      ],
    });
    return {
      name,
      kind: 'PDF from code',
      concurrency: 1,
      make: async () => pdfmake,
      render: async (lib, doc) => new Uint8Array(await lib.createPdf(definition(doc)).getBuffer()),
      close: () => {},
      sizes: { warm: 50, burst: 200 },
    };
  };
}

// @react-pdf/renderer ---------------------------------------------------------------------------
drivers['react-pdf'] = async () => {
  const React = require('react');
  const { Document, Page, View, Text, Font, renderToBuffer } = await import('@react-pdf/renderer');
  Font.register({ family: 'Noto', fonts: [{ src: FONT }, { src: FONT_BOLD, fontWeight: 'bold' }] });
  const h = React.createElement;
  const cell = (width, align, extra = {}) => ({ width, textAlign: align, ...extra });
  const row = (key, cells, style = {}) =>
    h(View, { key, wrap: false, style: { flexDirection: 'row', padding: '3 6', borderBottom: '0.5 solid #e5e7eb', ...style } }, ...cells);
  const tree = ({ data }) =>
    h(
      Document,
      null,
      h(
        Page,
        { size: 'LETTER', style: { padding: 32, fontFamily: 'Noto', fontSize: 9, color: '#1f2937' } },
        h(View, { style: { flexDirection: 'row', justifyContent: 'space-between' } }, h(Text, { style: { fontSize: 16, fontWeight: 'bold', color: '#4f46e5' } }, 'Lands Horizon\n'), h(Text, { style: { fontWeight: 'bold', textAlign: 'right' } }, `${data.title}\n#${data.no}\n24 Oct 2026`)),
        h(Text, { style: { marginVertical: 8 } }, 'Billed to Acme Corp'),
        ...data.sections.flatMap((s, k) => [
          ...(s.heading ? [h(Text, { key: `h${k}`, style: { fontSize: 12, fontWeight: 'bold', color: '#3730a3', marginTop: 10, marginBottom: 4 } }, s.heading)] : []),
          h(
            View,
            { key: `t${k}` },
            row('head', ['Description', 'Qty', 'Unit', 'Amount'].map((t, i) => h(Text, { key: t, style: cell(['58%', '10%', '14%', '18%'][i], i ? 'right' : 'left', { fontWeight: 'bold', color: '#3730a3' }) }, t)), { backgroundColor: '#eef2ff' }),
            ...s.rows.map((r, i) => row(i, [h(Text, { key: 'n', style: cell('58%', 'left') }, r.name), h(Text, { key: 'q', style: cell('10%', 'right') }, String(r.qty)), h(Text, { key: 'u', style: cell('14%', 'right') }, `₱${r.price}`), h(Text, { key: 'a', style: cell('18%', 'right') }, `₱${r.qty * r.price}`)])),
            ...(data.sections.length > 1 ? [row('sub', [h(Text, { key: 's', style: cell('82%', 'left', { fontWeight: 'bold' }) }, 'Subtotal'), h(Text, { key: 'v', style: cell('18%', 'right', { fontWeight: 'bold' }) }, `₱${s.subtotal}`)])] : []),
          ),
        ]),
        h(Text, { style: { textAlign: 'right', fontWeight: 'bold', fontSize: 12, marginTop: 10 } }, `Total ₱${data.total}`),
      ),
    );
  return {
    name: '@react-pdf/renderer',
    kind: 'PDF from code',
    concurrency: 1,
    make: async () => null,
    render: async (_, doc) => new Uint8Array(await renderToBuffer(tree(doc))),
    close: () => {},
    sizes: { warm: 20, burst: 60 },
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
