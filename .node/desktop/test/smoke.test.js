'use strict';
// Smoke: the package loads, the engine starts and makes a PDF. If any of these fail, nothing else matters.
const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const pkg = require('..');
const { binary, isPdf, pageCount, withEngine } = require('./helpers');

test('the native binary exists (run `cargo build` first)', () => {
  assert.ok(fs.existsSync(binary), `${binary} is missing`);
});

test('the package exports the public API', () => {
  assert.equal(typeof pkg.PDFEngine, 'function');
  assert.equal(typeof pkg.PDFEngineError, 'function');
  assert.equal(typeof pkg.PROTOCOL_VERSION, 'number');
  for (const method of ['generate', 'printers', 'print', 'ping', 'loadFonts', 'reloadFonts', 'close']) {
    assert.equal(typeof pkg.PDFEngine.prototype[method], 'function', `PDFEngine.${method}`);
  }
});

test('the engine starts, speaks the expected protocol and reports a version', () =>
  withEngine(async (pdf) => {
    const info = await pdf.ping();
    assert.match(info.version, /^\d+\.\d+\.\d+/);
    assert.equal(info.protocol, pkg.PROTOCOL_VERSION);
    assert.ok(Number.isInteger(pdf.pid) && pdf.pid > 0);
  }));

test('the smallest document: a Buffer with one page', () =>
  withEngine(async (pdf) => {
    const bytes = await pdf.generate({ template: '<p>Hello</p>' });
    assert.ok(isPdf(bytes));
    assert.equal(pageCount(bytes), 1);
    assert.match(bytes.subarray(-8).toString('latin1'), /%%EOF/);
  }));

test('close() stops the engine process, and is safe to call twice', async () => {
  await withEngine(async (pdf) => {
    await pdf.ping();
    const pid = pdf.pid;
    await pdf.close();
    await pdf.close();
    await new Promise((r) => setTimeout(r, 200));
    assert.throws(() => process.kill(pid, 0), /ESRCH|EPERM/, 'the process is gone');
  });
});
