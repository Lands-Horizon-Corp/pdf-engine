// Shared by the functional suites (smoke, happy-path, sad-path, limits).
import assert from 'node:assert/strict';
import { PDFEngine } from '../index.js';

export const latin1 = (bytes) => Buffer.from(bytes).toString('latin1');
export const isPdf = (bytes) => bytes instanceof Uint8Array && latin1(bytes.subarray(0, 5)) === '%PDF-';
export const pageCount = (bytes) => (latin1(bytes).match(/\/Type\s*\/Page(?![s\w])/g) || []).length;

/** Runs `body(pdf)` with a fresh engine and always closes it. */
export async function withEngine(body, options) {
  const pdf = new PDFEngine(options);
  try {
    return await body(pdf);
  } finally {
    pdf.close();
  }
}

/** Asserts that the engine still works: a plain document renders to one page. */
export async function assertHealthy(pdf) {
  const bytes = await pdf.generate({ template: '<p>still alive {{ n }}</p>', data: { n: 1 } });
  assert.ok(isPdf(bytes), 'the engine still makes PDFs');
  assert.equal(pageCount(bytes), 1);
}
