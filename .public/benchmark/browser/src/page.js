// Runs inside the browser: renders the same documents with each library and reports timings.
// The libraries: pdf-engine (WebAssembly), and the two that photograph the page, html2pdf.js and jspdf-html2canvas.
import { PDFEngine } from '@lands-horizon-corp/pdf-engine-wasm';
import wasmUrl from '@lands-horizon-corp/pdf-engine-wasm/wasm?url';
import html2pdf from 'html2pdf.js';
import jsPDFHtml2canvas from 'jspdf-html2canvas';
import { scenarios } from '../../scenarios.js';

const engine = new PDFEngine({ wasm: wasmUrl });

/** The document in the page, as the two screenshot libraries need it (they draw what is on screen). */
function host(doc) {
  const el = document.createElement('div');
  el.style.cssText = 'width:794px;background:#fff';
  el.innerHTML = `<style>${doc.css}</style>${doc.body}`;
  document.body.appendChild(el);
  return el;
}

const libraries = {
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
