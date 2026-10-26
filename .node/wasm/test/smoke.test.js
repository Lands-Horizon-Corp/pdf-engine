// Smoke: the package loads, starts and makes a PDF. If any of these fail, nothing else matters.
import test from 'node:test';
import assert from 'node:assert/strict';
import * as pkg from '../index.js';
import { PDFEngine, PDFEngineError } from '../index.js';
import { isPdf, pageCount, withEngine } from './helpers.js';

test('the package exports the public API', () => {
  assert.equal(typeof PDFEngine, 'function');
  assert.equal(typeof PDFEngineError, 'function');
  for (const method of ['generate', 'download', 'open', 'print', 'preview', 'fromElement', 'ping', 'loadFonts', 'close']) {
    assert.equal(typeof PDFEngine.prototype[method], 'function', `PDFEngine.${method}`);
  }
  assert.ok(Object.keys(pkg).length >= 2);
});

test('the engine starts and reports a version', () =>
  withEngine(async (pdf) => {
    assert.match((await pdf.ping()).version, /^\d+\.\d+\.\d+/);
  }));

test('the smallest document: a PDF with one page', () =>
  withEngine(async (pdf) => {
    const bytes = await pdf.generate({ template: '<p>Hello</p>' });
    assert.ok(isPdf(bytes));
    assert.equal(pageCount(bytes), 1);
    assert.match(Buffer.from(bytes.subarray(-8)).toString('latin1'), /%%EOF/);
  }));

test('the bytes are a Uint8Array that can be copied and re-read', () =>
  withEngine(async (pdf) => {
    const bytes = await pdf.generate({ template: '<p>Hello</p>' });
    assert.ok(bytes instanceof Uint8Array);
    const copy = new Uint8Array(bytes);
    assert.deepEqual(copy, bytes);
    bytes.fill(0); // changing what we were given must not break the next document
    assert.ok(isPdf(await pdf.generate({ template: '<p>Again</p>' })));
  }));

test('a server-side-only call needs no WebAssembly: errors are PDFEngineError', () =>
  withEngine(async (pdf) => {
    await assert.rejects(pdf.generate({}), PDFEngineError);
  }));

test('close() is safe to call twice', async () => {
  const pdf = new PDFEngine();
  await pdf.ping();
  pdf.close();
  assert.doesNotThrow(() => pdf.close());
});
