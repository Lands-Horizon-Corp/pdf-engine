'use strict';
// Run by windows-printer-test.ps1: print a receipt to a virtual printer and check what arrived.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { PDFEngine } = require('../../.node/desktop');

const name = process.env.PDF_ENGINE_TEST_PRINTER;
const out = process.env.PDF_ENGINE_TEST_OUT;
const exe = path.resolve(__dirname, '..', '..', 'target', 'debug', 'pdf-engine.exe');

(async () => {
  const pdf = new PDFEngine({ binary: process.env.PDF_ENGINE_BIN || exe });
  try {
    const printers = await pdf.printers();
    console.log(printers.map((p) => `${p.id} [${p.type}, ${p.status}]`).join('\n'));
    const mine = printers.find((p) => p.id === `system://${name}`);
    assert.ok(mine, `printers() lists ${name}`);
    assert.equal(mine.type, 'thermal', 'a POS-58 queue is a receipt printer');

    const result = await pdf.print({
      printerId: mine.id,
      paper: '58mm',
      template: '<div style="font-family:monospace"><b>STORE</b>{% for i in items %}<div>{{ i }} ₱10</div>{% endfor %}<b>TOTAL ₱20</b></div>',
      data: { items: ['Sardines', 'Coffee'] },
    });
    assert.equal(result.language, 'escpos');

    // The spooler writes the file a moment later.
    const deadline = Date.now() + 60_000;
    while (!(fs.existsSync(out) && fs.statSync(out).size >= result.bytes) && Date.now() < deadline) await new Promise((r) => setTimeout(r, 500));
    const bytes = fs.readFileSync(out);
    console.log(`received ${bytes.length} of ${result.bytes} bytes`);
    assert.equal(bytes.length, result.bytes, 'every byte reached the printer');
    assert.deepEqual([...bytes.subarray(0, 2)], [0x1b, 0x40], 'starts with ESC @');
    assert.deepEqual([...bytes.subarray(2, 6)], [0x1d, 0x76, 0x30, 0x00], 'then a raster image');
    assert.deepEqual([...bytes.subarray(-4)], [0x1d, 0x56, 0x42, 0x00], 'ends with the cut');
    console.log('ok');
  } finally {
    await pdf.close();
  }
})().catch((e) => {
  console.error(e);
  process.exit(1);
});
