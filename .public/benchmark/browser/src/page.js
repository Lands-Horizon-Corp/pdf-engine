// Runs inside the browser: renders the same documents with each library and reports timings.
import { PDFEngine } from '@lands-horizon-corp/pdf-engine-wasm';
import wasmUrl from '@lands-horizon-corp/pdf-engine-wasm/wasm?url';
import html2pdf from 'html2pdf.js';
import jsPDFHtml2canvas from 'jspdf-html2canvas';
import { jsPDF } from 'jspdf';
import { PDFDocument as PdfLibDocument, StandardFonts, rgb } from 'pdf-lib';
import fontkit from '@pdf-lib/fontkit';
import PDFKit, { registerStdFonts } from 'pdfkit';
import helvetica from 'pdfkit/standard-fonts/Helvetica';
import helveticaBold from 'pdfkit/standard-fonts/HelveticaBold';
import pdfmakeHelvetica from 'pdfmake/build/standard-fonts/Helvetica.js?raw';
import pdfMake from 'pdfmake/build/pdfmake';
import { layout, PAGE } from '../../layout.js';
import { scenarios } from '../../scenarios.js';
import regularUrl from '../../../assets/fonts/NotoSans-Regular.ttf?url';
import boldUrl from '../../../assets/fonts/NotoSans-Bold.ttf?url';

// pdfkit in a browser ships no fonts until they are registered (it always opens Helvetica first).
registerStdFonts(helvetica, helveticaBold);

// pdfmake carries its own copy of pdfkit, so it gets the built-in fonts through its own container.
pdfMake.addFontContainer(new Function(`${pdfmakeHelvetica};return fontContainer`)());

const engine = new PDFEngine({ wasm: wasmUrl });

function host(doc) {
  const el = document.createElement('div');
  el.style.cssText = 'width:794px;background:#fff';
  el.innerHTML = `<style>${doc.css}</style>${doc.body}`;
  document.body.appendChild(el);
  return el;
}

// The Unicode font for the libraries that draw from code (they all use the same file). It is fetched once,
// like pdf-engine fetches its own fonts on its first call.
let noto = null;
const toB64 = (bytes) => {
  let out = '';
  for (let i = 0; i < bytes.length; i += 0x8000) out += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(out);
};
async function fonts() {
  if (!noto) {
    const get = async (url) => new Uint8Array(await (await fetch(url)).arrayBuffer());
    const [regular, bold] = await Promise.all([get(regularUrl), get(boldUrl)]);
    noto = { regular, bold, regular64: toB64(regular), bold64: toB64(bold) };
    pdfMake.addVirtualFileSystem({ 'Noto-Regular.ttf': noto.regular64, 'Noto-Bold.ttf': noto.bold64 });
    pdfMake.setFonts({
      Noto: { normal: 'Noto-Regular.ttf', bold: 'Noto-Bold.ttf' },
      Helvetica: { normal: 'Helvetica', bold: 'Helvetica-Bold', italics: 'Helvetica-Oblique', bolditalics: 'Helvetica-BoldOblique' },
    });
  }
  return noto;
}
const hex = (h) => [1, 3, 5].map((i) => parseInt(h.slice(i, i + 2), 16) / 255);

const jspdfDraw = (unicode) => async (doc) => {
  const f = unicode ? await fonts() : null;
  const pdf = new jsPDF({ unit: 'pt', format: 'letter', compress: true });
  const family = unicode ? 'Noto' : 'helvetica';
  if (unicode) {
    pdf.addFileToVFS('Noto-Regular.ttf', f.regular64).addFont('Noto-Regular.ttf', 'Noto', 'normal');
    pdf.addFileToVFS('Noto-Bold.ttf', f.bold64).addFont('Noto-Bold.ttf', 'Noto', 'bold');
  }
  layout(doc.data, unicode ? '₱' : 'PHP ').forEach((ops, index) => {
    if (index > 0) pdf.addPage();
    for (const op of ops) {
      if (op.k === 'text') {
        pdf.setFont(family, op.bold ? 'bold' : 'normal').setFontSize(op.size).setTextColor(op.color);
        pdf.text(op.t, op.align === 'right' ? op.x + op.w : op.x, op.y + op.size * 0.85, { align: op.align });
      } else if (op.k === 'rect') pdf.setFillColor(op.color).rect(op.x, op.y, op.w, op.h, 'F');
      else pdf.setDrawColor(op.color).setLineWidth(0.5).line(op.x1, op.y, op.x2, op.y);
    }
  });
  return new Uint8Array(pdf.output('arraybuffer'));
};

const pdfLibDraw = (unicode) => async (doc) => {
  const f = unicode ? await fonts() : null;
  const pdf = await PdfLibDocument.create();
  let regular;
  let bold;
  if (unicode) {
    pdf.registerFontkit(fontkit);
    regular = await pdf.embedFont(f.regular, { subset: true });
    bold = await pdf.embedFont(f.bold, { subset: true });
  } else {
    regular = await pdf.embedFont(StandardFonts.Helvetica);
    bold = await pdf.embedFont(StandardFonts.HelveticaBold);
  }
  for (const ops of layout(doc.data, unicode ? '₱' : 'PHP ')) {
    const page = pdf.addPage([PAGE.w, PAGE.h]);
    for (const op of ops) {
      const color = rgb(...hex(op.color));
      if (op.k === 'text') {
        const font = op.bold ? bold : regular;
        const width = op.align === 'right' ? font.widthOfTextAtSize(op.t, op.size) : 0;
        page.drawText(op.t, { x: op.align === 'right' ? op.x + op.w - width : op.x, y: PAGE.h - op.y - op.size, size: op.size, font, color });
      } else if (op.k === 'rect') page.drawRectangle({ x: op.x, y: PAGE.h - op.y - op.h, width: op.w, height: op.h, color });
      else page.drawLine({ start: { x: op.x1, y: PAGE.h - op.y }, end: { x: op.x2, y: PAGE.h - op.y }, thickness: 0.5, color });
    }
  }
  return pdf.save();
};

const pdfkitDraw = (unicode) => async (doc) => {
  const f = unicode ? await fonts() : null;
  const [regular, bold, peso] = unicode ? ['Noto', 'NotoBold', '₱'] : ['Helvetica', 'Helvetica-Bold', 'PHP '];
  return new Promise((ok, fail) => {
    const pdf = new PDFKit({ size: 'LETTER', margin: 0, autoFirstPage: false });
    if (unicode) pdf.registerFont('Noto', f.regular).registerFont('NotoBold', f.bold);
    const chunks = [];
    pdf
      .on('data', (c) => chunks.push(c))
      .on('end', () => {
        const out = new Uint8Array(chunks.reduce((n, c) => n + c.length, 0));
        let offset = 0;
        for (const c of chunks) {
          out.set(c, offset);
          offset += c.length;
        }
        ok(out);
      })
      .on('error', fail);
    for (const ops of layout(doc.data, peso)) {
      pdf.addPage({ size: 'LETTER', margin: 0 });
      for (const op of ops) {
        if (op.k === 'text') pdf.font(op.bold ? bold : regular).fontSize(op.size).fillColor(op.color).text(op.t, op.x, op.y, { width: op.w, align: op.align, lineBreak: false });
        else if (op.k === 'rect') pdf.rect(op.x, op.y, op.w, op.h).fill(op.color);
        else pdf.moveTo(op.x1, op.y).lineTo(op.x2, op.y).lineWidth(0.5).strokeColor(op.color).stroke();
      }
    }
    pdf.end();
  });
};

// The same document definition as the Node benchmark (.public/benchmark/libs.mjs).
const pdfmakeDraw = (unicode) => async ({ data }) => {
  await fonts();
  const [font, peso] = unicode ? ['Noto', '₱'] : ['Helvetica', 'PHP '];
  const right = (text, extra = {}) => ({ text: String(text), alignment: 'right', ...extra });
  const definition = {
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
  };
  return new Uint8Array(await pdfMake.createPdf(definition).getBuffer());
};

const libraries = {
  'jsPDF (Noto Sans)': jspdfDraw(true),
  'jsPDF (built-in font)': jspdfDraw(false),
  'pdf-lib (Noto Sans)': pdfLibDraw(true),
  'pdf-lib (built-in font)': pdfLibDraw(false),
  'pdfkit (Noto Sans)': pdfkitDraw(true),
  'pdfkit (built-in font)': pdfkitDraw(false),
  'pdfmake (Noto Sans)': pdfmakeDraw(true),
  'pdfmake (built-in font)': pdfmakeDraw(false),
  'pdf-engine (WebAssembly)': (doc) => engine.generate({ template: doc.html }),
  'html2pdf.js': async (doc, el) =>
    new Uint8Array(await html2pdf().set({ margin: 0, jsPDF: { unit: 'in', format: 'letter' } }).from(el).outputPdf('arraybuffer')),
  'jspdf-html2canvas': async (doc, el) => {
    const pdf = await jsPDFHtml2canvas(el, { jsPDF: { format: 'letter' }, success: () => {} });
    return new Uint8Array(pdf.output('arraybuffer'));
  },
};

function toBase64(bytes) {
  let s = '';
  for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(s);
}

window.bench = {
  libraries: Object.keys(libraries),
  async run(library, docName, runs, warm = 0) {
    const doc = scenarios[docName].render(0);
    const el = host(doc);
    try {
      const once = async () => {
        const started = performance.now();
        const bytes = await libraries[library](doc, el);
        return [performance.now() - started, bytes];
      };
      const [first, firstBytes] = await once();
      // Browsers compile hot code in stages (WebAssembly too); the timed runs start after a warm-up, as a
      // server or a busy page would see them. The first call above is reported separately.
      for (let i = 0; i < warm; i++) await once();
      const times = [];
      let last = firstBytes;
      for (let i = 0; i < runs; i++) {
        const [ms, bytes] = await once();
        times.push(ms);
        last = bytes;
      }
      return { first, times, bytes: toBase64(last) };
    } finally {
      el.remove();
    }
  },
};
// Many documents in a row (the scale benchmark): each gets its own number; stops at `count` or the time budget.
window.bench.batch = async (library, scenarioName, count, budgetMs) => {
  const scenario = scenarios[scenarioName];
  const render = async (i) => {
    const doc = scenario.render(i);
    const el = host(doc);
    try {
      return await libraries[library](doc, el);
    } finally {
      el.remove();
    }
  };
  for (let i = 0; i < 3; i++) await render(i); // warm-up, not counted
  const started = performance.now();
  let completed = 0;
  let errors = 0;
  let firstError = null;
  let first = null;
  for (let i = 0; i < count && performance.now() - started < budgetMs; i++) {
    try {
      const bytes = await render(i);
      if (completed === 0) first = toBase64(bytes);
      completed++;
    } catch (e) {
      errors++;
      firstError ??= String(e?.message || e);
    }
  }
  return { completed, errors, firstError, ms: performance.now() - started, first };
};
window.benchReady = true;
