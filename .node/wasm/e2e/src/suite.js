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

    // fromElement(): what is on screen becomes the document.
    const stage = document.createElement('div');
    stage.id = 'stage';
    stage.innerHTML = `<style>
        #stage .card { display: flex; gap: 12px; padding: 14px; border: 2px solid #2563eb; border-radius: 10px; background: #eff6ff; font: 16px/1.4 Georgia, serif; width: 520px; }
        #stage .card .grow { flex: 1; color: #1d4ed8; }
        #stage .tag::before { content: "ID "; font-weight: 700; }
        #stage table { border-collapse: collapse } #stage td { border: 1px solid #999; padding: 4px 8px }
      </style>
      <div class="card"><div class="grow"><b>Order {{ 42 }}</b> <span class="tag">7</span><div style="display:none">hidden</div></div>
        <canvas id="cv" width="40" height="40"></canvas>
        <svg width="40" height="40" viewBox="0 0 40 40"><circle cx="20" cy="20" r="16" class="dot"/></svg>
        <input value="typed text"><input type="checkbox" checked><script>window.__ran = true<\/script></div>
      <table><tr><td>a</td><td>b</td></tr></table>`;
    document.body.append(stage);
    const ctx = stage.querySelector('#cv').getContext('2d');
    ctx.fillStyle = '#e11d48';
    ctx.fillRect(0, 0, 40, 40);
    const extra = document.createElement('style');
    extra.textContent = '#stage .dot { fill: #16a34a }';
    document.head.append(extra);
    const options = await pdf.fromElement('#stage', { password: undefined });
    const raw = options.template;
    result.fromElement = {
      literal: raw.startsWith('{% raw %}') && raw.endsWith('{% endraw %}'),
      styled: /display:\s*flex/.test(raw) && /border-top-width:2px/.test(raw),
      text: raw.includes('Order {{ 42 }}'),
      canvas: /<img src="data:image\/png;base64,/.test(raw),
      svgPaint: /fill:\s*rgb\(22, 163, 74\)/.test(raw),
      pseudo: raw.includes('>ID <'),
      hiddenGone: !raw.includes('hidden'),
      noScript: !raw.includes('<script'),
      inputText: raw.includes('typed text'),
      pageSize: /^\d+px$/.test(options.width) && /^\d+px$/.test(options.height),
    };
    const fromScreen = await pdf.generate(options);
    result.fromElement.pdf = text(fromScreen.subarray(0, 5));
    result.fromElement.hasText = /NotoSans/.test(text(fromScreen));
    result.fromElement.template = '{{ 42 }} was not evaluated';
    stage.remove();
    extra.remove();

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
