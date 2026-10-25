'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const net = require('node:net');
const fs = require('node:fs');
const { classify, listPrinters, parseLpstat, parsePrinterId, parseWindowsPrinters, sendNet, sendSystemPdf, sendSystemRaw } = require('../printing.js');

test('classify: receipt, label and office printers by name', () => {
  for (const name of ['POS-58 Thermal Receipt Printer', 'EPSON TM-T82', 'Star TSP143', 'Rongta RP326', 'XP-80C', 'GP-58130']) assert.equal(classify(name), 'thermal', name);
  for (const name of ['Zebra ZD420', 'TSC TTP-244', 'Label Printer', 'Brother QL-800']) assert.equal(classify(name), 'label', name);
  for (const name of ['HP LaserJet P1102', 'Canon PIXMA', 'Office A4 Invoice Printer']) assert.equal(classify(name), 'document', name);
});

test('parseLpstat reads CUPS output', () => {
  const printers = parseLpstat({
    printers: ['printer POS58_USB is idle.  enabled since Sat Oct 25 09:00:00 2026', 'printer HP_LaserJet_P1102 disabled since Sat Oct 25 08:00:00 2026 -', 'printer Kitchen_TM_T82 now printing Kitchen_TM_T82-12.  enabled since Sat'].join('\n'),
    devices: ['device for POS58_USB: usb://POS/POS58?serial=1', 'device for HP_LaserJet_P1102: ipp://printer.local/ipp/print', 'device for Kitchen_TM_T82: socket://192.168.1.150:9100'].join('\n'),
    defaults: 'system default destination: HP_LaserJet_P1102',
  });
  assert.deepEqual(
    printers.map((p) => [p.id, p.type, p.status, p.default]),
    [
      ['system://POS58_USB', 'thermal', 'ready', false],
      ['system://HP_LaserJet_P1102', 'document', 'offline', true],
      ['system://Kitchen_TM_T82', 'thermal', 'busy', false],
    ],
  );
  assert.equal(printers[0].uri, 'usb://POS/POS58?serial=1');
  assert.deepEqual(parseLpstat({}), []);
});

test('parseWindowsPrinters reads Get-Printer JSON (one printer or many)', () => {
  const many = parseWindowsPrinters(JSON.stringify([{ Name: 'POS-58', DriverName: 'Generic / Text Only', PortName: 'USB001', PrinterStatus: 0 }, { Name: 'HP LaserJet', DriverName: 'HP', PortName: 'IP_1', PrinterStatus: 3 }]));
  assert.deepEqual(many.map((p) => [p.id, p.type, p.status]), [['system://POS-58', 'thermal', 'ready'], ['system://HP LaserJet', 'document', 'offline']]);
  assert.equal(parseWindowsPrinters(JSON.stringify({ Name: 'Zebra ZD420', PrinterStatus: 'Normal' }))[0].type, 'label');
  assert.deepEqual(parseWindowsPrinters(''), []);
  assert.deepEqual(parseWindowsPrinters('not json'), []);
});

test('parsePrinterId', () => {
  assert.deepEqual(parsePrinterId('system://My Printer'), { scheme: 'system', name: 'My Printer' });
  assert.deepEqual(parsePrinterId('net://192.168.1.150:9100'), { scheme: 'net', host: '192.168.1.150', port: 9100 });
  assert.deepEqual(parsePrinterId('net://kitchen.local'), { scheme: 'net', host: 'kitchen.local', port: 9100 });
  for (const bad of ['http://printer', 'printer', '', undefined, 'net://']) assert.throws(() => parsePrinterId(bad), { code: 'INVALID_FIELD' }, String(bad));
});

test('listPrinters: system printers plus the ones you list, with a missing spooler tolerated', async () => {
  const exec = (cmd, args, opts, cb) => {
    const out = { '-p': 'printer POS58 is idle.  enabled since x', '-v': 'device for POS58: usb://x', '-d': 'no system default destination' }[args[0]];
    cb(null, out, '');
  };
  const list = await listPrinters({ platform: 'darwin', exec, usb: false, extra: [{ id: 'net://192.168.1.150:9100', name: 'Kitchen Epson TM-T82' }] });
  assert.deepEqual(list.map((p) => [p.id, p.type, p.transport]), [['net://192.168.1.150:9100', 'thermal', 'net'], ['system://POS58', 'thermal', 'system']]);
  const broken = (cmd, args, opts, cb) => cb(Object.assign(new Error('spawn lpstat ENOENT'), { code: 'ENOENT' }), '', '');
  assert.deepEqual(await listPrinters({ platform: 'linux', exec: broken }), []);
  assert.equal((await listPrinters({ platform: 'linux', exec: broken, usb: false, extra: [{ id: 'net://h' }] })).length, 1);
});

test('sendNet writes the bytes to the printer port', async () => {
  const received = [];
  const server = net.createServer((socket) => socket.on('data', (c) => received.push(c)));
  await new Promise((ok) => server.listen(0, '127.0.0.1', ok));
  try {
    const bytes = Buffer.from([0x1b, 0x40, 1, 2, 3, 0x1d, 0x56, 0x42, 0]);
    const result = await sendNet('127.0.0.1', server.address().port, bytes);
    await new Promise((ok) => setTimeout(ok, 50));
    assert.deepEqual(Buffer.concat(received), bytes);
    assert.equal(result.bytes, bytes.length);
    await assert.rejects(sendNet('127.0.0.1', 1, bytes, { timeoutMs: 500 }), { code: 'PRINTER_UNREACHABLE' });
  } finally {
    server.close();
  }
});

test('system printing hands the job to lp', async () => {
  const calls = [];
  const exec = (cmd, args, opts, cb) => {
    calls.push({ cmd, args, content: fs.existsSync(args.at(-1)) ? fs.readFileSync(args.at(-1)) : null });
    cb(null, 'request id is x-1', '');
  };
  await sendSystemRaw('POS58', Buffer.from('RAW'), { platform: 'darwin', exec });
  assert.deepEqual(calls[0].args.slice(0, 4), ['-d', 'POS58', '-o', 'raw']);
  assert.equal(calls[0].content.toString(), 'RAW');
  await sendSystemPdf('HP LaserJet', Buffer.from('%PDF-x'), { copies: 2, platform: 'linux', exec });
  assert.deepEqual(calls[1].args.slice(0, 4), ['-d', 'HP LaserJet', '-n', '2']);
  assert.equal(calls[1].content.toString(), '%PDF-x');
});

// A stand-in for the `usb` package (WebUSB API): one receipt printer, one keyboard.
function fakeUsb({ failClaimOnce = false, transferStatus = 'ok' } = {}) {
  const log = { transfers: [], calls: [] };
  let claimFailures = failClaimOnce ? 1 : 0;
  const printer = {
    vendorId: 0x0483, productId: 0x5740, productName: 'POS-58', manufacturerName: 'Zjiang', serialNumber: 'S1',
    configuration: null,
    configurations: [{ configurationValue: 1, interfaces: [{ interfaceNumber: 0, alternates: [{ alternateSetting: 0, interfaceClass: 7, endpoints: [{ endpointNumber: 1, direction: 'in', type: 'bulk' }, { endpointNumber: 2, direction: 'out', type: 'bulk' }] }] }] }],
    async open() { log.calls.push('open'); },
    async close() { log.calls.push('close'); },
    async selectConfiguration(n) { log.calls.push(`config ${n}`); this.configuration = { configurationValue: n }; },
    async claimInterface(n) { if (claimFailures-- > 0) throw new Error('LIBUSB_ERROR_ACCESS busy'); log.calls.push(`claim ${n}`); },
    async detachKernelDriver(n) { log.calls.push(`detach ${n}`); },
    async releaseInterface(n) { log.calls.push(`release ${n}`); },
    async transferOut(endpoint, data) { log.transfers.push({ endpoint, length: data.length }); return { status: transferStatus, bytesWritten: data.length }; },
  };
  const keyboard = { vendorId: 0x046d, productId: 0xc31c, productName: 'Keyboard', configurations: [{ configurationValue: 1, interfaces: [{ interfaceNumber: 0, alternates: [{ interfaceClass: 3, endpoints: [{ endpointNumber: 1, direction: 'in', type: 'interrupt' }] }] }] }] };
  return { module: { async getDevices() { return [keyboard, printer]; } }, log };
}

test('parsePrinterId: usb ids are hexadecimal vendor:product', () => {
  assert.deepEqual(parsePrinterId('usb://0483:5740'), { scheme: 'usb', vendorId: 0x0483, productId: 0x5740 });
  for (const bad of ['usb://0483', 'usb://zzzz:5740', 'usb://']) assert.throws(() => parsePrinterId(bad), { code: 'INVALID_FIELD' }, bad);
});

test('listPrinters offers USB receipt printers and ignores other devices', async () => {
  const { module } = fakeUsb();
  const broken = (cmd, args, opts, cb) => cb(Object.assign(new Error('no lpstat'), { code: 'ENOENT' }), '', '');
  const list = await listPrinters({ platform: 'linux', exec: broken, usbModule: module });
  assert.deepEqual(list.map((p) => [p.id, p.name, p.type, p.transport]), [['usb://0483:5740', 'POS-58', 'thermal', 'usb']]);
  // `usb: false` skips it; a missing `usb` package is not an error.
  assert.deepEqual(await listPrinters({ platform: 'linux', exec: broken, usb: false, usbModule: module }), []);
  assert.deepEqual(await listPrinters({ platform: 'linux', exec: broken, usbModule: { async getDevices() { throw new Error('no access'); } } }), []);
});

test('sendUsb claims the printer interface, writes in chunks, and always cleans up', async () => {
  const { sendUsb } = require('../usb.js');
  const { module, log } = fakeUsb();
  const result = await sendUsb(0x0483, 0x5740, Buffer.alloc(40_000, 1), { usbModule: module });
  assert.equal(result.bytes, 40_000);
  assert.deepEqual(log.transfers, [{ endpoint: 2, length: 16384 }, { endpoint: 2, length: 16384 }, { endpoint: 2, length: 7232 }]);
  assert.deepEqual(log.calls, ['open', 'config 1', 'claim 0', 'release 0', 'close']);

  // Linux: a kernel driver holds it; it is detached and the claim retried.
  const busy = fakeUsb({ failClaimOnce: true });
  await sendUsb(0x0483, 0x5740, Buffer.from('x'), { usbModule: busy.module });
  assert.ok(busy.log.calls.includes('detach 0') && busy.log.calls.includes('claim 0'));

  // A failed transfer is a PRINT_FAILED, and the device is still released and closed.
  const bad = fakeUsb({ transferStatus: 'stall' });
  await assert.rejects(sendUsb(0x0483, 0x5740, Buffer.from('x'), { usbModule: bad.module }), { code: 'PRINT_FAILED', message: /stall/ });
  assert.deepEqual(bad.log.calls.slice(-2), ['release 0', 'close']);

  await assert.rejects(sendUsb(0x1111, 0x2222, Buffer.from('x'), { usbModule: module }), { code: 'PRINTER_UNREACHABLE' });
});
