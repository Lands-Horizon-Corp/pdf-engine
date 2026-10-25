'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const net = require('node:net');
const path = require('node:path');
const { PDFEngine, PDFEngineError } = require('..');

const exe = process.platform === 'win32' ? '.exe' : '';
const binary = process.env.PDF_ENGINE_BIN || path.resolve(__dirname, `../../../target/debug/pdf-engine${exe}`);

/** A fake network printer: resolves with what the first connection sent. */
async function fakePrinter() {
  let resolveBytes;
  const received = new Promise((r) => (resolveBytes = r));
  const server = net.createServer((socket) => {
    const chunks = [];
    socket.on('data', (c) => chunks.push(c));
    socket.on('end', () => resolveBytes(Buffer.concat(chunks)));
  });
  await new Promise((ok) => server.listen(0, '127.0.0.1', ok));
  return { port: server.address().port, received, close: () => server.close() };
}

test('printers() lists the ones you name, and print() sends the receipt to them', async () => {
  const printer = await fakePrinter();
  const id = `net://127.0.0.1:${printer.port}`;
  const pdf = new PDFEngine({ binary, printers: [{ id, name: 'Kitchen Epson TM-T82' }] });
  try {
    const list = await pdf.printers();
    const kitchen = list.find((p) => p.id === id);
    assert.deepEqual([kitchen.name, kitchen.type, kitchen.transport], ['Kitchen Epson TM-T82', 'thermal', 'net']);

    const result = await pdf.print({
      printerId: id,
      paper: '80mm',
      template: '<b>STORE</b>{% for i in items %}<div>{{ i }} ₱10</div>{% endfor %}',
      data: { items: ['Sardines', 'Coffee'] },
      cut: false,
    });
    assert.deepEqual([result.language, result.type, result.copies], ['escpos', 'thermal', 1]);

    const bytes = await printer.received;
    assert.equal(bytes.length, result.bytes);
    assert.deepEqual([...bytes.subarray(0, 6)], [0x1b, 0x40, 0x1d, 0x76, 0x30, 0x00]);
    assert.equal(bytes.readUInt16LE(6), 72, '80 mm is 576 dots: 72 bytes per row');
    assert.notDeepEqual([...bytes.subarray(-4)], [0x1d, 0x56, 0x42, 0x00], 'cut: false leaves out the cut');
  } finally {
    printer.close();
    await pdf.close();
  }
});

test('print() errors are PDFEngineErrors with codes', async () => {
  const pdf = new PDFEngine({ binary });
  try {
    await assert.rejects(pdf.print({ printerId: 'net://127.0.0.1:1', template: '<b>x</b>' }), (e) => e instanceof PDFEngineError && e.code === 'PRINTER_UNREACHABLE' && e.status === 502);
    await assert.rejects(pdf.print({ printerId: 'lpt1', template: 'x' }), { code: 'INVALID_FIELD' });
    await assert.rejects(pdf.print({ template: 'x' }), { code: 'MISSING_FIELD' });
    await assert.rejects(pdf.print(), { code: 'MISSING_FIELD' });
  } finally {
    await pdf.close();
  }
});
