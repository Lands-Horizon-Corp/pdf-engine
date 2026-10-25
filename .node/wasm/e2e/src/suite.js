// The checks every app runs in the browser. Results are read by tests/browsers.spec.js.
import { PDFEngine, PDFEngineError } from '@lands-horizon-corp/pdf-engine-wasm';
import wasmUrl from '@lands-horizon-corp/pdf-engine-wasm/wasm?url';

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
