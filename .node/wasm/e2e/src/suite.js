// The checks every app runs in the browser. Results are read by tests/browsers.spec.js.
import { PDFEngine, PDFEngineError } from '@lands-horizon-corp/pdf-engine-wasm';
import wasmUrl from '@lands-horizon-corp/pdf-engine-wasm/wasm?url';
import { createPDFEngine } from '@lands-horizon-corp/pdf-engine-wasm/svelte';

export { wasmUrl };

const text = (bytes) => new TextDecoder('latin1').decode(bytes);
const PNG =
  'iVBORw0KGgoAAAANSUhEUgAAAAoAAAAKCAYAAACNMs+9AAAAFUlEQVR42mP8z8BQz0AEYBxVSF+FABJADveWkH6oAAAAAElFTkSuQmCC';

export async function runSuite() {
  const pdf = new PDFEngine({ wasm: wasmUrl });
  const result = {};
  try {
    const started = performance.now();
    const invoice = await pdf.generate({
      template: '<h1>Invoice {{ no }}</h1><p>₱1,234.50 Привет Γειά <b>bold</b></p>',
      data: { no: '12345' },
    });
    result.invoice = {
      header: text(invoice.subarray(0, 5)),
      bytes: invoice.length,
      latinFont: /NotoSans-Regular/.test(text(invoice)),
      boldFont: /NotoSans-Bold/.test(text(invoice)),
      firstCallMs: Math.round(performance.now() - started),
    };

    const encrypted = await pdf.generate({ template: '<p>secret</p>', password: 'hunter2' });
    result.encrypted = /\/Encrypt/.test(text(encrypted));

    const receipt = text(await pdf.generate({ template: '<p>x</p>', width: '80mm', height: '200mm' }));
    result.receiptPage = /\/MediaBox\s*\[\s*0\s+0\s+226\.7\d*\s+566\.9\d*\s*\]/.test(receipt);

    const withImage = text(await pdf.generate({ template: `<img src="data:image/png;base64,${PNG}" width="40" height="40">` }));
    result.image = /\/Subtype\s*\/Image/.test(withImage);

    try {
      await pdf.generate({ template: '<p>\n{% if %}' });
      result.error = null;
    } catch (e) {
      result.error = { code: e.code, status: e.status, line: e.line, isEngineError: e instanceof PDFEngineError };
    }

    // Rendering on a deployed server from another origin (needs CORS_ALLOWED_ORIGINS).
    try {
      const remote = new PDFEngine({ url: 'http://127.0.0.1:4181', token: 't', wasm: '/no/such.wasm' });
      const bytes = await remote.generate({ template: '<p>{{ v }}</p>', data: { v: 'from the server' } });
      result.remote = text(bytes.subarray(0, 5));
      await remote.generate({ template: '<p>\n{% if %}' }).catch((e) => (result.remoteError = e.code));
    } catch (e) {
      result.remote = `failed: ${e.code} ${e.message}`;
    }

    // `font-family: Arial, sans-serif` must still draw text (a browser has no system fonts).
    result.genericFont = /NotoSans-Regular/.test(text(await pdf.generate({ template: '<p style="font-family: Arial, sans-serif">Hello</p>' })));

    // Documents made in Web Workers.
    const pool = new PDFEngine({ wasm: wasmUrl, workers: 2 });
    try {
      const made = await Promise.all(Array.from({ length: 6 }, (_, i) => pool.generate({ template: '<p>{{ i }}</p>{% pagebreak %}<p>two</p>', data: { i } })));
      result.workers = {
        ok: made.filter((b) => text(b.subarray(0, 5)) === '%PDF-').length,
        pages: (text(made[0]).match(/\/Type\s*\/Page(?![s\w])/g) || []).length,
        error: await pool.generate({ template: '<p>\n{% if %}' }).then(() => null, (e) => ({ code: e.code, line: e.line, isEngineError: e instanceof PDFEngineError })),
      };
    } finally {
      pool.close();
    }

    // The Svelte store: subscribe, make a document, see busy go up and down, and an error land in the store.
    const store = createPDFEngine({ wasm: wasmUrl });
    const seen = [];
    const stop = store.subscribe((s) => seen.push(`${s.busy}:${s.error ? s.error.code : '-'}`));
    const viaStore = await store.generate({ template: '<p>store</p>' });
    await store.generate({ template: '<p>\n{% if %}' }).catch(() => {});
    stop();
    store.close();
    result.store = { header: text(viaStore.subarray(0, 5)), states: seen };

    // The live preview, with outlines, in a target element.
    const host = document.createElement('div');
    host.id = 'preview-host';
    document.body.append(host);
    const preview = await pdf.preview({ template: '<h1>Preview {{ n }}</h1>', data: { n: 1 } }, { target: '#preview-host' });
    const first = preview.url;
    await preview.update({ template: '<h1>Preview {{ n }}</h1>', data: { n: 2 } });
    result.preview = { frame: host.querySelector('iframe') === preview.element, urlChanged: !!first && preview.url !== first && preview.url.startsWith('blob:') };
    preview.close();
    result.previewClosed = !host.querySelector('iframe');

    let ok = 0;
    for (let i = 0; i < 20; i++) {
      const bytes = await pdf.generate({ template: '<p>{{ i }}</p>', data: { i } });
      if (text(bytes.subarray(0, 5)) === '%PDF-') ok++;
    }
    result.repeated = ok;
  } catch (e) {
    result.fatal = `${e?.code ?? ''} ${e?.message ?? e}`;
  } finally {
    pdf.close();
  }
  return result;
}

export function publish(result) {
  window.__result = result;
  const out = document.getElementById('result');
  if (out) out.textContent = JSON.stringify(result, null, 2);
}
