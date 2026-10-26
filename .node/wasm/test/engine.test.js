import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import { PDFEngine, PDFEngineError } from '../index.js';

// Run `npm run build` first.
const latin1 = (bytes) => Buffer.from(bytes).toString('latin1');
const isPdf = (bytes) => bytes instanceof Uint8Array && latin1(bytes.subarray(0, 5)) === '%PDF-';

test('generate() returns the PDF bytes', async () => {
  const pdf = new PDFEngine();
  try {
    const bytes = await pdf.generate({ template: '<h1>Invoice {{ no }}</h1>', data: { no: '12345' }, password: '' });
    assert.ok(isPdf(bytes));
    assert.ok(bytes.length > 500);
    assert.match((await pdf.ping()).version, /^\d+\.\d+\.\d+/);
  } finally {
    pdf.close();
  }
});

test('the bundled fonts draw Latin, Cyrillic and Greek, and the currency sign', async () => {
  const pdf = new PDFEngine();
  try {
    const bytes = await pdf.generate({ template: '<p>₱1,234.50 Привет Γειά <b>bold</b></p>' });
    assert.match(latin1(bytes), /NotoSans/);
    assert.match(latin1(bytes), /NotoSans-Bold/);
  } finally {
    pdf.close();
  }
});

test('page size and orientation are honoured', async () => {
  const pdf = new PDFEngine();
  try {
    const receipt = latin1(await pdf.generate({ template: '<p>x</p>', width: '80mm', height: '200mm' }));
    assert.match(receipt, /\/MediaBox\s*\[\s*0\s+0\s+226\.7\d*\s+566\.9\d*\s*\]/);
    const wide = latin1(await pdf.generate({ template: '<p>x</p>', width: '100mm', height: '50mm', orientation: 'landscape' }));
    assert.match(wide, /\/MediaBox\s*\[\s*0\s+0\s+(5\d\d|[2-9]\d\d)\.\d+\s+/);
  } finally {
    pdf.close();
  }
});

test('a password encrypts the PDF', async () => {
  const pdf = new PDFEngine();
  try {
    const bytes = await pdf.generate({ template: '<p>secret</p>', password: 'hunter2' });
    assert.ok(isPdf(bytes));
    assert.match(latin1(bytes), /\/Encrypt/);
  } finally {
    pdf.close();
  }
});

test('data: URI images are embedded', async () => {
  const png =
    'iVBORw0KGgoAAAANSUhEUgAAAAoAAAAKCAYAAACNMs+9AAAAFUlEQVR42mP8z8BQz0AEYBxVSF+FABJADveWkH6oAAAAAElFTkSuQmCC';
  const pdf = new PDFEngine();
  try {
    const withImage = await pdf.generate({ template: `<img src="data:image/png;base64,${png}" width="40" height="40">` });
    const without = await pdf.generate({ template: '<p>no image</p>' });
    assert.match(latin1(withImage), /\/Subtype\s*\/Image/);
    assert.doesNotMatch(latin1(without), /\/Subtype\s*\/Image/);
    // A broken image never costs the document.
    const broken = await pdf.generate({ template: '<p>hi</p><img src="data:image/png;base64,AAAA">' });
    assert.ok(isPdf(broken));
  } finally {
    pdf.close();
  }
});

test('errors are PDFEngineError with a code, status, hint and line', async () => {
  const pdf = new PDFEngine();
  try {
    await assert.rejects(pdf.generate({ template: '<p>\n{% if %}' }), (e) => {
      assert.ok(e instanceof PDFEngineError);
      assert.equal(e.code, 'TEMPLATE_ERROR');
      assert.equal(e.status, 400);
      assert.equal(e.line, 2);
      assert.match(e.message, /Template rendering failed/);
      assert.ok(e.hint);
      return true;
    });
    await assert.rejects(pdf.generate({ template: 'x', width: 'wide' }), { code: 'INVALID_FIELD', status: 400 });
    await assert.rejects(pdf.generate({}), { code: 'MISSING_FIELD' });
    await assert.rejects(
      pdf.generate({ template: '{% for a in range(100000) %}{% for b in range(100000) %}{% endfor %}{% endfor %}' }),
      { code: 'TEMPLATE_TOO_EXPENSIVE', status: 400 },
    );
    // Still usable after errors.
    assert.ok(isPdf(await pdf.generate({ template: '<p>ok</p>' })));
  } finally {
    pdf.close();
  }
});

test('templates escape HTML in data', async () => {
  const pdf = new PDFEngine();
  try {
    const bytes = await pdf.generate({ template: '<p>{{ v }}</p>', data: { v: '<script>alert(1)</script>' } });
    assert.ok(isPdf(bytes)); // rendered as text, not parsed as a tag
  } finally {
    pdf.close();
  }
});

test('fonts: bytes, blank entries and defaultFonts: false', async () => {
  const bold = new Uint8Array(fs.readFileSync(new URL('../fonts/NotoSans-Bold.ttf', import.meta.url)));
  const custom = new PDFEngine({ fonts: ['', bold], defaultFonts: false });
  try {
    const out = latin1(await custom.generate({ template: '<p>only bold is available</p>' }));
    assert.match(out, /NotoSans-Bold/);
    assert.doesNotMatch(out, /NotoSans-Regular/);
  } finally {
    custom.close();
  }
});

test('two engines with different fonts do not interfere', async () => {
  const boldOnly = new Uint8Array(fs.readFileSync(new URL('../fonts/NotoSans-Bold.ttf', import.meta.url)));
  const a = new PDFEngine();
  const b = new PDFEngine({ fonts: [boldOnly], defaultFonts: false });
  try {
    for (let i = 0; i < 2; i++) {
      assert.match(latin1(await a.generate({ template: '<p>regular</p>' })), /NotoSans-Regular/);
      assert.doesNotMatch(latin1(await b.generate({ template: '<p>regular</p>' })), /NotoSans-Regular/);
    }
  } finally {
    a.close();
    b.close();
  }
});

test('a font that cannot be loaded is a FONT_LOAD_FAILED error', async () => {
  const pdf = new PDFEngine({ fonts: ['/no/such/font.ttf'] });
  await assert.rejects(pdf.generate({ template: '<p>x</p>' }), { code: 'FONT_LOAD_FAILED' });
});

test('a wasm file that cannot be loaded is a WASM_LOAD_FAILED error', async () => {
  const pdf = new PDFEngine({ wasm: '/no/such/file.wasm' });
  await assert.rejects(pdf.generate({ template: '<p>x</p>' }), { code: 'WASM_LOAD_FAILED' });
  // ...and the next engine still works.
  const ok = new PDFEngine();
  assert.ok(isPdf(await ok.generate({ template: '<p>x</p>' })));
  ok.close();
});

test('a closed engine says so', async () => {
  const pdf = new PDFEngine();
  await pdf.ping();
  pdf.close();
  await assert.rejects(pdf.generate({ template: 'x' }), { code: 'ENGINE_CLOSED' });
});

test('many documents in a row', async () => {
  const pdf = new PDFEngine();
  try {
    for (let i = 0; i < 30; i++) {
      assert.ok(isPdf(await pdf.generate({ template: '<p>{{ i }}</p>', data: { i } })));
    }
  } finally {
    pdf.close();
  }
});

// ---- rendering on a deployed server (`url`) ----------------------------------------------
import { spawn } from 'node:child_process';
import { createServer } from 'node:net';

const serverBinary =
  process.env.PDF_ENGINE_BIN ||
  new URL(`../../../target/debug/pdf-engine${process.platform === 'win32' ? '.exe' : ''}`, import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, '$1');

async function freePort() {
  return new Promise((resolve) => {
    const s = createServer().listen(0, '127.0.0.1', () => {
      const { port } = s.address();
      s.close(() => resolve(port));
    });
  });
}

async function withServer(fn) {
  const port = await freePort();
  const child = spawn(serverBinary, [], {
    env: { ...process.env, API_PORT: String(port), API_BEARER_TOKEN: 'secret' },
    stdio: 'ignore',
    cwd: os.tmpdir(),
  });
  try {
    for (let i = 0; i < 100; i++) {
      try {
        if ((await fetch(`http://127.0.0.1:${port}/health`)).ok) break;
      } catch {
        /* not up yet */
      }
      await new Promise((r) => setTimeout(r, 100));
    }
    return await fn(`http://127.0.0.1:${port}`);
  } finally {
    child.kill();
  }
}

const hasServer = fs.existsSync(serverBinary);

test('url: renders on the server and never loads WebAssembly', { skip: !hasServer && 'build the native binary first (cargo build)' }, async () => {
  await withServer(async (url) => {
    const pdf = new PDFEngine({ url, token: 'secret', wasm: '/no/such/file.wasm' }); // a bad wasm path proves it is unused
    const bytes = await pdf.generate({ template: '<h1>Invoice {{ no }}</h1>', data: { no: '12345' }, password: '' });
    assert.ok(isPdf(bytes));
    // Per-document override, and the same options as local rendering.
    const local = new PDFEngine();
    const receipt = await local.generate({ template: '<p>x</p>', width: '80mm', height: '200mm', url, token: 'secret' });
    assert.match(latin1(receipt), /\/MediaBox\s*\[\s*0\s+0\s+226\.7\d*\s+566\.9\d*\s*\]/);
    local.close();
  });
});

test('url: server errors keep their code, status, hint and line', { skip: !hasServer && 'build the native binary first (cargo build)' }, async () => {
  await withServer(async (url) => {
    const pdf = new PDFEngine({ url, token: 'secret' });
    await assert.rejects(pdf.generate({ template: '<p>\n{% if %}' }), { code: 'TEMPLATE_ERROR', status: 400, line: 2 });
    await assert.rejects(pdf.generate({ template: 'x', width: 'wide' }), { code: 'INVALID_FIELD' });
    await assert.rejects(new PDFEngine({ url, token: 'wrong' }).generate({ template: 'x' }), { code: 'UNAUTHORIZED', status: 401 });
    await assert.rejects(new PDFEngine({ url }).generate({ template: 'x' }), { code: 'UNAUTHORIZED' });
    // A token that already says "Bearer" works too.
    assert.ok(isPdf(await new PDFEngine({ url, token: 'Bearer secret' }).generate({ template: '<p>ok</p>' })));
  });
});

test('url: an unreachable server is SERVER_UNREACHABLE', async () => {
  const pdf = new PDFEngine({ url: 'http://127.0.0.1:1' });
  await assert.rejects(pdf.generate({ template: 'x' }), (e) => {
    assert.equal(e.code, 'SERVER_UNREACHABLE');
    assert.match(e.hint, /CORS/);
    return true;
  });
});

const PNG = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAoAAAAKCAYAAACNMs+9AAAAFUlEQVR42mP8z8BQz0AEYBxVSF+FABJADveWkH6oAAAAAElFTkSuQmCC', 'base64');
const hasImage = (bytes) => /\/Subtype\s*\/Image/.test(latin1(bytes));

test('images: bytes, Blob, URL and data: URI in the `images` map, and fetched URLs', async () => {
  const pdf = new PDFEngine();
  const seen = [];
  const fetchImpl = async (url) => {
    seen.push(String(url));
    return String(url).includes('missing') ? new Response('nope', { status: 404, statusText: 'Not Found' }) : new Response(PNG);
  };
  try {
    const template = '<img src="logo" width="40"><img src="https://cdn.test/a.png" width="40">';
    const skipped = [];
    const pdfBytes = await pdf.generate({
      template,
      images: { logo: PNG },
      fetch: fetchImpl,
      onImageError: (src, why) => skipped.push([src, why]),
    });
    assert.ok(hasImage(pdfBytes));
    assert.deepEqual(seen, ['https://cdn.test/a.png']);
    assert.equal(skipped.length, 0);

    for (const logo of [new Blob([PNG]), new Uint8Array(PNG), `data:image/png;base64,${PNG.toString('base64')}`]) {
      assert.ok(hasImage(await pdf.generate({ template: '<img src="logo" width="40">', images: { logo } })));
    }

    // A missing image is skipped with a reason and the document still renders.
    const bad = await pdf.generate({
      template: '<p>hi</p><img src="https://cdn.test/missing.png">',
      fetch: fetchImpl,
      onImageError: (src, why) => skipped.push([src, why]),
    });
    assert.ok(isPdf(bad) && !hasImage(bad));
    assert.match(skipped[0][1], /404/);

    // Not an image: skipped too.
    const text = await pdf.generate({ template: '<img src="x">', images: { x: new TextEncoder().encode('hello') }, onImageError: (s, w) => skipped.push([s, w]) });
    assert.ok(isPdf(text) && !hasImage(text));
  } finally {
    pdf.close();
  }
});

test('images: a template cannot read local files unless allowed', async () => {
  const file = `${os.tmpdir()}/pdf-engine-img-${process.pid}.png`;
  fs.writeFileSync(file, PNG);
  const pdf = new PDFEngine();
  try {
    const reasons = [];
    const template = `<img src="${file}" width="40">`;
    const denied = await pdf.generate({ template, onImageError: (s, w) => reasons.push(w) });
    assert.ok(!hasImage(denied));
    assert.match(reasons[0], /allowLocalFiles/);
    assert.ok(hasImage(await pdf.generate({ template, allowLocalFiles: true })));
  } finally {
    fs.rmSync(file, { force: true });
    pdf.close();
  }
});

test('compression: fast stores streams, small is smaller, anything else is rejected', async () => {
  const pdf = new PDFEngine();
  try {
    const template = '<p>Привет Γειά ₱1,234</p>'.repeat(40);
    const fast = await pdf.generate({ template });
    const small = await pdf.generate({ template, compression: 'small' });
    assert.ok(isPdf(fast) && isPdf(small));
    assert.ok(small.length < fast.length, `${small.length} < ${fast.length}`);
    // The choice is per call: back to the default.
    assert.equal((await pdf.generate({ template })).length, fast.length);
    await assert.rejects(pdf.generate({ template, compression: 'max' }), (e) => e.code === 'INVALID_FIELD');
  } finally {
    pdf.close();
  }
});

const pageCount = (bytes) => (latin1(bytes).match(/\/Type\s*\/Page(?![s\w])/g) || []).length;

test('partials: include, extends and import, from the constructor and per call', async () => {
  const pdf = new PDFEngine({ partials: { header: '<h1>{{ title }}</h1>' } });
  try {
    assert.equal(pageCount(await pdf.generate({ template: '{% include "header" %}<p>x</p>', data: { title: 'T' } })), 1);
    const layout = '<header>{% block head %}{% endblock %}</header>{% pagebreak %}<p>end</p>';
    const extended = await pdf.generate({
      template: '{% extends "layout" %}{% block head %}{{ title }}{% endblock %}',
      data: { title: 'T' },
      partials: { layout },
    });
    assert.equal(pageCount(extended), 2, 'the per-call partial and its pagebreak are used');
    // The constructor's partial is still there on the next call, the per-call one is not.
    await assert.rejects(pdf.generate({ template: '{% include "layout" %}' }), { code: 'TEMPLATE_ERROR' });
    await assert.rejects(pdf.generate({ template: 'x', partials: { '../bad': 'x' } }), { code: 'INVALID_FIELD' });
  } finally {
    pdf.close();
  }
});

test('{% pagebreak %} starts a new page, and <barcode> draws vector shapes', async () => {
  const pdf = new PDFEngine();
  try {
    assert.equal(pageCount(await pdf.generate({ template: '<p>a</p><p>b</p>' })), 1);
    assert.equal(pageCount(await pdf.generate({ template: '<p>a</p>{% pagebreak %}<p>b</p>' })), 2);
    const plain = await pdf.generate({ template: '<p>x</p>' });
    const qr = await pdf.generate({ template: '<p>x</p><barcode value="https://example.com/{{ id }}" width="120"/>', data: { id: 42 } });
    assert.ok(qr.length > plain.length, 'the QR code adds drawing operations');
    assert.doesNotMatch(latin1(qr), /\/Subtype\s*\/Image/, 'vector shapes, not a picture');
    await assert.rejects(pdf.generate({ template: '<barcode value="12AB" type="ean13"/>' }), { code: 'INVALID_FIELD' });
  } finally {
    pdf.close();
  }
});

test('workers: documents are made in worker threads, errors and images included', async () => {
  const pdf = new PDFEngine({ workers: 2, partials: { header: '<h1>{{ t }}</h1>' } });
  const skipped = [];
  try {
    assert.match((await pdf.ping()).version, /^\d+\.\d+\.\d+/);
    // Several at once, from one call site.
    const many = await Promise.all(Array.from({ length: 8 }, (_, i) => pdf.generate({ template: '{% include "header" %}<p>{{ i }}</p>{% pagebreak %}<p>x</p>', data: { t: 'T', i } })));
    for (const bytes of many) assert.ok(isPdf(bytes) && pageCount(bytes) === 2);
    assert.ok(bytes_differ(many[0], many[1]), 'each document has its own data');
    // Errors keep their codes.
    await assert.rejects(pdf.generate({ template: '<p>\n{% if %}' }), { code: 'TEMPLATE_ERROR', line: 2 });
    await assert.rejects(pdf.generate({ template: 'x', compression: 'max' }), { code: 'INVALID_FIELD' });
    // An image supplied as bytes; a missing one is reported to this call's callback.
    const png = Uint8Array.from(Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAoAAAAKCAYAAACNMs+9AAAAFUlEQVR42mP8z8BQz0AEYBxVSF+FABJADveWkH6oAAAAAElFTkSuQmCC', 'base64'));
    const withImage = await pdf.generate({ template: '<img src="logo" width="40"><img src="https://nope.invalid/x.png">', images: { logo: png }, fetch: undefined, onImageError: (src, why) => skipped.push([src, why]) });
    assert.match(latin1(withImage), /\/Subtype\s*\/Image/);
    assert.equal(skipped.length, 1);
    assert.match(skipped[0][0], /nope\.invalid/);
    // Fonts reach every worker.
    await pdf.loadFonts([]);
    assert.ok(isPdf(await pdf.generate({ template: '<p>still works</p>' })));
  } finally {
    pdf.close();
  }
  await assert.rejects(pdf.generate({ template: 'x' }), { code: 'ENGINE_CLOSED' });
  await assert.rejects(new PDFEngine({ workers: 99 }).generate({ template: 'x' }), { code: 'INVALID_FIELD' });
});

const bytes_differ = (a, b) => Buffer.compare(Buffer.from(a), Buffer.from(b)) !== 0;

test('@font-face: a font in a data: URI, or fetched from a URL, is used for that document only', async () => {
  const ttf = fs.readFileSync(new URL('../fonts/NotoSans-Bold.ttf', import.meta.url));
  const uri = `data:font/ttf;base64,${ttf.toString('base64')}`;
  const text = `<p style="font-family: 'Brand Display'; font-size: 24px">Hello World</p>`;
  // No bundled fonts: the only font there can be is the one the document brings.
  const pdf = new PDFEngine({
    defaultFonts: false,
    fetch: async (url) => {
      assert.equal(String(url), 'https://fonts.test/brand.ttf');
      return new Response(ttf);
    },
  });
  try {
    const none = await pdf.generate({ template: text });
    assert.doesNotMatch(latin1(none), /NotoSans/, 'without the rule there is no font');

    const fromData = await pdf.generate({ template: `<style>@font-face { font-family: 'Brand Display'; src: url(${uri}) format('truetype'); }</style>${text}` });
    assert.match(latin1(fromData), /NotoSans-Bold/);

    const fromUrl = await pdf.generate({ template: `<style>@font-face { font-family: 'Brand Display'; src: url(https://fonts.test/brand.ttf) format('truetype'); }</style>${text}` });
    assert.match(latin1(fromUrl), /NotoSans-Bold/);

    const after = await pdf.generate({ template: text });
    assert.doesNotMatch(latin1(after), /NotoSans/, 'the font belongs to the document that declared it');

    // A font that cannot be fetched never costs the document, and `onImageError` says why.
    const reasons = [];
    const broken = await pdf.generate({
      template: `<style>@font-face { font-family: 'Brand Display'; src: url(https://fonts.test/missing.ttf) }</style>${text}`,
      fetch: async () => new Response('nope', { status: 404, statusText: 'Not Found' }),
      onImageError: (src, why) => reasons.push(why),
    });
    assert.ok(isPdf(broken));
    assert.match(reasons.join(), /font: 404/);
  } finally {
    pdf.close();
  }
});

test('markdown: a filter and a block turn Markdown into the document, and raw HTML in it stays text', async () => {
  const pdf = new PDFEngine();
  try {
    const plain = await pdf.generate({ template: '<p>x</p>' });
    const md = await pdf.generate({
      template: '{{ notes|markdown }}{% filter markdown %}## Title\n\n| a | b |\n|---|---|\n| 1 | 2 |\n{% endfilter %}',
      data: { notes: '# Hello\n\n**bold** and <script>alert(1)</script>\n\n- one\n- two' },
    });
    assert.ok(isPdf(md));
    assert.match(latin1(md), /NotoSans-Bold/, 'headings and bold text are set in bold');
    assert.ok(md.length > plain.length);
  } finally {
    pdf.close();
  }
});
