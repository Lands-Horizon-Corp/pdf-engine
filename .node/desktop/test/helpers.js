'use strict';
// Shared by the functional suites (smoke, happy-path, sad-path, limits, sidecar).
const assert = require('node:assert/strict');
const path = require('node:path');
const { PDFEngine } = require('..');

const exe = process.platform === 'win32' ? '.exe' : '';
const binary = process.env.PDF_ENGINE_BIN || path.resolve(__dirname, `../../../target/debug/pdf-engine${exe}`);

const isPdf = (buf) => Buffer.isBuffer(buf) && buf.subarray(0, 5).toString() === '%PDF-';
const pageCount = (buf) => (buf.toString('latin1').match(/\/Type\s*\/Page(?![s\w])/g) || []).length;

/** Runs `body(pdf)` with an engine on the test binary and always closes it. */
async function withEngine(body, options = {}) {
  const pdf = new PDFEngine({ binary, ...options });
  try {
    return await body(pdf);
  } finally {
    await pdf.close();
  }
}

/** Asserts that the engine still works: a plain document renders to one page. */
async function assertHealthy(pdf) {
  const bytes = await pdf.generate({ template: '<p>still alive {{ n }}</p>', data: { n: 1 } });
  assert.ok(isPdf(bytes), 'the engine still makes PDFs');
  assert.equal(pageCount(bytes), 1);
}

module.exports = { binary, isPdf, pageCount, withEngine, assertHealthy };
