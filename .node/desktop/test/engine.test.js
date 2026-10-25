'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { PDFEngine, PDFEngineError, PROTOCOL_VERSION } = require('..');

// Run `cargo build` (or `npm run build:native -- --debug`) first; PDF_ENGINE_BIN overrides the path.
const exe = process.platform === 'win32' ? '.exe' : '';
const binary = process.env.PDF_ENGINE_BIN || path.resolve(__dirname, `../../../target/debug/pdf-engine${exe}`);

const isPdf = (buf) => Buffer.isBuffer(buf) && buf.subarray(0, 5).toString() === '%PDF-';

test('generate() returns the PDF bytes', async () => {
  const pdf = new PDFEngine({ binary });
  try {
    const bytes = await pdf.generate({
      template: '<h1>Invoice {{ no }}</h1>',
      data: { no: '12345' },
      password: '',
    });
    assert.ok(isPdf(bytes), 'starts with %PDF-');
    assert.ok(bytes.length > 500);
  } finally {
    await pdf.close();
  }
});

test('versions match and page options are honoured', async () => {
  const pdf = new PDFEngine({ binary });
  try {
    const info = await pdf.ping();
    assert.equal(info.protocol, PROTOCOL_VERSION);
    assert.match(info.version, /^\d+\.\d+\.\d+/);

    const receipt = await pdf.generate({ template: '<p>x</p>', width: '80mm', height: '200mm' });
    // 80mm x 200mm in points: 226.77 x 566.93
    const text = receipt.toString('latin1');
    assert.match(text, /\/MediaBox\s*\[\s*0\s+0\s+226\.7\d*\s+566\.9\d*\s*\]/);
  } finally {
    await pdf.close();
  }
});

test('an encrypted PDF is produced when a password is given', async () => {
  const pdf = new PDFEngine({ binary });
  try {
    const bytes = await pdf.generate({ template: '<p>secret</p>', password: 'hunter2' });
    assert.ok(isPdf(bytes));
    assert.match(bytes.toString('latin1'), /\/Encrypt/);
  } finally {
    await pdf.close();
  }
});

test('errors are PDFEngineError with a code, status, hint and line', async () => {
  const pdf = new PDFEngine({ binary });
  try {
    await assert.rejects(
      pdf.generate({ template: '<p>\n{% if %}' }),
      (e) => {
        assert.ok(e instanceof PDFEngineError);
        assert.equal(e.code, 'TEMPLATE_ERROR');
        assert.equal(e.status, 400);
        assert.equal(e.line, 2);
        assert.match(e.message, /Template rendering failed/);
        assert.ok(e.hint);
        return true;
      },
    );
    await assert.rejects(pdf.generate({ template: 'x', width: 'wide' }), { code: 'INVALID_FIELD', status: 400 });
    await assert.rejects(pdf.generate({}), { code: 'MISSING_FIELD' });
    // The engine is still usable after errors.
    assert.ok(isPdf(await pdf.generate({ template: '<p>ok</p>' })));
  } finally {
    await pdf.close();
  }
});

test('many concurrent generate() calls each get their own PDF', async () => {
  const pdf = new PDFEngine({ binary });
  try {
    const results = await Promise.all(
      Array.from({ length: 40 }, (_, i) => pdf.generate({ template: '<p>{{ i }}</p>', data: { i } })),
    );
    assert.equal(results.length, 40);
    for (const bytes of results) assert.ok(isPdf(bytes));
  } finally {
    await pdf.close();
  }
});

test('a large PDF arrives intact across many stream chunks', async () => {
  const pdf = new PDFEngine({ binary });
  try {
    const rows = Array.from({ length: 4000 }, (_, i) => ({ id: i, name: `Item ${i}`, price: i * 1.5 }));
    const bytes = await pdf.generate({
      template: '<table>{% for r in rows %}<tr><td>{{ r.id }}</td><td>{{ r.name }}</td><td>{{ r.price }}</td></tr>{% endfor %}</table>',
      data: { rows },
    });
    assert.ok(isPdf(bytes));
    assert.ok(bytes.length > 100_000, `only ${bytes.length} bytes`);
    assert.match(bytes.subarray(-1024).toString('latin1'), /%%EOF\s*$/);
  } finally {
    await pdf.close();
  }
});

test('fonts: bad paths are reported with FONT_ERROR', async () => {
  const pdf = new PDFEngine({ binary, fonts: ['/definitely/not/here.ttf'] });
  try {
    await assert.rejects(pdf.generate({ template: '<p>x</p>' }), { code: 'FONT_ERROR' });
  } finally {
    await pdf.close();
  }
});

test('fonts: an empty list, empty strings and a folder without fonts', async () => {
  const empty = fs.mkdtempSync(path.join(os.tmpdir(), 'pdf-engine-fonts-'));
  const ok = new PDFEngine({ binary, fonts: [''] }); // `[""]` is ignored
  try {
    assert.ok(isPdf(await ok.generate({ template: '<p>x</p>' })));
  } finally {
    await ok.close();
  }
  const none = new PDFEngine({ binary, fonts: [empty] });
  try {
    await assert.rejects(none.generate({ template: '<p>x</p>' }), { code: 'FONT_ERROR' });
  } finally {
    await none.close();
    fs.rmSync(empty, { recursive: true });
  }
});

test('a missing binary is a clear BINARY_NOT_FOUND error', async () => {
  const pdf = new PDFEngine({ binary: '/no/such/pdf-engine' });
  await assert.rejects(pdf.generate({ template: '<p>x</p>' }), (e) => {
    assert.equal(e.code, 'BINARY_NOT_FOUND');
    assert.match(e.message, /\/no\/such\/pdf-engine/);
    assert.ok(e.hint);
    return true;
  });
});

test('the engine restarts after it dies', async () => {
  const pdf = new PDFEngine({ binary });
  try {
    await pdf.ping();
    // Kill the child behind the client's back.
    const { spawnSync } = require('node:child_process');
    if (process.platform === 'win32') spawnSync('taskkill', ['/F', '/IM', 'pdf-engine.exe', '/T'], { stdio: 'ignore' });
    else spawnSync('pkill', ['-f', `${binary} ipc`], { stdio: 'ignore' }); // no shell: it would match itself
    await new Promise((r) => setTimeout(r, 300));
    assert.ok(isPdf(await pdf.generate({ template: '<p>again</p>' })));
  } finally {
    await pdf.close();
  }
});

test('close() lets requests in progress finish and rejects new ones', async () => {
  const pdf = new PDFEngine({ binary });
  const inFlight = pdf.generate({ template: '<p>last</p>' });
  await pdf.ping();
  const closing = pdf.close();
  assert.ok(isPdf(await inFlight));
  await closing;
  await assert.rejects(pdf.generate({ template: 'x' }), { code: 'ENGINE_CLOSED' });
});

const PNG = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAoAAAAKCAYAAACNMs+9AAAAFUlEQVR42mP8z8BQz0AEYBxVSF+FABJADveWkH6oAAAAAElFTkSuQmCC', 'base64');
const hasImage = (bytes) => /\/Subtype\s*\/Image/.test(bytes.toString('latin1'));

test('images: supplied as bytes, a data: URI or a file, and a folder of images', async () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'pdf-engine-img-'));
  fs.writeFileSync(path.join(dir, 'logo.png'), PNG);
  const pdf = new PDFEngine({ binary, imageDir: dir });
  try {
    const template = '<img src="a"><img src="b"><img src="c">';
    const bytes = await pdf.generate({
      template,
      images: { a: PNG, b: `data:image/png;base64,${PNG.toString('base64')}`, c: path.join(dir, 'logo.png') },
    });
    assert.ok(isPdf(bytes) && hasImage(bytes));

    // From imageDir, with no height given.
    assert.ok(hasImage(await pdf.generate({ template: '<img src="logo.png" width="60">' })));
    // Outside imageDir, and not an image: skipped, the document still renders.
    assert.ok(!hasImage(await pdf.generate({ template: '<img src="../x.png"><img src="t">', images: { t: Buffer.from('text') } })));

    await assert.rejects(pdf.generate({ template: '<img src="a">', images: { a: 42 } }), (e) => e instanceof PDFEngineError && e.code === 'INVALID_FIELD');
    await assert.rejects(pdf.generate({ template: '<img src="a">', images: { a: path.join(dir, 'nope.png') } }), (e) => e.code === 'INVALID_FIELD');
  } finally {
    await pdf.close();
    fs.rmSync(dir, { recursive: true, force: true });
  }
});
