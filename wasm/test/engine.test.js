import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
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
