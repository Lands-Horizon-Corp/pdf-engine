'use strict';
// The command line and the wire protocol, as a Tauri sidecar (or any other language) uses them.
const test = require('node:test');
const assert = require('node:assert/strict');
const { spawn, spawnSync } = require('node:child_process');
const { binary } = require('./helpers');

/** One frame: [u32 header_len][u32 body_len][header][body], big-endian. */
function frame(header, body = Buffer.alloc(0)) {
  const h = Buffer.from(typeof header === 'string' ? header : JSON.stringify(header));
  const out = Buffer.alloc(8 + h.length + body.length);
  out.writeUInt32BE(h.length, 0);
  out.writeUInt32BE(body.length, 4);
  h.copy(out, 8);
  body.copy(out, 8 + h.length);
  return out;
}

/** Runs `pdf-engine ipc`, writes `input`, and resolves with the frames it answers and its exit status. */
function session(input, { endInput = true, wait = 10_000 } = {}) {
  return new Promise((resolve, reject) => {
    const child = spawn(binary, ['ipc'], { stdio: ['pipe', 'pipe', 'ignore'] });
    const chunks = [];
    child.stdout.on('data', (c) => chunks.push(c));
    const timer = setTimeout(() => {
      child.kill();
      reject(new Error('the engine did not answer in time'));
    }, wait);
    child.on('close', (code) => {
      clearTimeout(timer);
      const buf = Buffer.concat(chunks);
      const frames = [];
      for (let at = 0; at + 8 <= buf.length; ) {
        const hl = buf.readUInt32BE(at);
        const bl = buf.readUInt32BE(at + 4);
        frames.push({ header: JSON.parse(buf.subarray(at + 8, at + 8 + hl).toString()), body: buf.subarray(at + 8 + hl, at + 8 + hl + bl) });
        at += 8 + hl + bl;
      }
      resolve({ frames, code });
    });
    child.stdin.on('error', () => {});
    child.stdin.write(input);
    if (endInput) child.stdin.end();
  });
}

test('ipc: ping, render and an error, answered by id (possibly out of order)', async () => {
  const { frames, code } = await session(Buffer.concat([
    frame({ id: 1, op: 'ping' }),
    frame({ id: 2, op: 'render', template: '<h1>{{ n }}</h1>', data: { n: 7 } }),
    frame({ id: 3, op: 'render', template: '{% if %}' }),
  ]));
  assert.equal(code, 0);
  const byId = Object.fromEntries(frames.map((f) => [f.header.id, f]));
  assert.equal(byId[1].header.ok, true);
  assert.equal(byId[1].header.protocol, 1);
  assert.equal(byId[2].header.ok, true);
  assert.equal(byId[2].body.subarray(0, 5).toString(), '%PDF-');
  assert.equal(byId[2].header.bytes, byId[2].body.length);
  assert.equal(byId[3].header.ok, false);
  assert.equal(byId[3].header.error.code, 'TEMPLATE_ERROR');
});

test('ipc: images travel in the body, and lengths that do not add up are IPC_PROTOCOL_ERROR', async () => {
  const png = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==', 'base64');
  const good = await session(frame({ id: 1, template: '<img src="logo" width="30">', images: [{ src: 'logo', len: png.length }] }, png));
  assert.equal(good.frames[0].header.ok, true);
  assert.match(good.frames[0].body.toString('latin1'), /\/Subtype\s*\/Image/);
  const bad = await session(frame({ id: 2, template: '<img src="logo">', images: [{ src: 'logo', len: png.length + 5 }] }, png));
  assert.equal(bad.frames[0].header.error.code, 'IPC_PROTOCOL_ERROR');
});

test('ipc: a field of the wrong type is INVALID_FIELD with the request id, not silence', async () => {
  const { frames } = await session(frame({ id: 9, template: 5 }));
  assert.equal(frames[0].header.id, 9);
  assert.equal(frames[0].header.error.code, 'INVALID_FIELD');
});

test('ipc: unknown ops and missing fields are answered with the id', async () => {
  const { frames } = await session(Buffer.concat([frame({ id: 1, op: 'no_such_op' }), frame({ id: 2, op: 'render' }), frame({ id: 3, op: 'load_fonts' })]));
  const byId = Object.fromEntries(frames.map((f) => [f.header.id, f.header]));
  assert.equal(byId[1].error.code, 'INVALID_FIELD');
  assert.equal(byId[2].error.code, 'MISSING_FIELD');
  assert.equal(byId[3].error.code, 'MISSING_FIELD');
});

test('ipc: a header that is not JSON ends the session with IPC_PROTOCOL_ERROR (id null)', async () => {
  const { frames } = await session(frame('this is not json'));
  assert.equal(frames[0].header.id, null);
  assert.equal(frames[0].header.ok, false);
  assert.equal(frames[0].header.error.code, 'IPC_PROTOCOL_ERROR');
});

test('ipc: a stream cut in the middle of a frame ends the session cleanly (exit status 2 and an error frame, or 0)', async () => {
  const whole = frame({ id: 1, template: '<p>x</p>' });
  const { frames, code } = await session(whole.subarray(0, whole.length - 5));
  assert.ok(code === 2 || code === 0, `exit status ${code}`);
  if (frames.length) assert.equal(frames[0].header.ok, false);
});

test('ipc: garbage bytes and an absurd frame length are refused without crashing or hanging', async () => {
  const garbage = await session(Buffer.from('GET / HTTP/1.1\r\n\r\n'));
  assert.ok(garbage.code !== null);
  const huge = Buffer.alloc(8);
  huge.writeUInt32BE(0xffffffff, 0);
  huge.writeUInt32BE(0xffffffff, 4);
  const absurd = await session(huge);
  assert.ok(absurd.code !== null);
  if (absurd.frames.length) assert.equal(absurd.frames[0].header.error.code, 'IPC_PROTOCOL_ERROR');
});

test('ipc: closing standard input finishes the requests in progress, then exits 0', async () => {
  const { frames, code } = await session(Buffer.concat(Array.from({ length: 20 }, (_, i) => frame({ id: i + 1, template: '<p>{{ i }}</p>', data: { i } }))));
  assert.equal(code, 0);
  assert.equal(frames.length, 20);
  assert.ok(frames.every((f) => f.header.ok));
});

test('ipc: a raster op returns ESC/POS and ZPL commands', async () => {
  const { frames } = await session(Buffer.concat([
    frame({ id: 1, op: 'raster', template: '<b>Receipt</b>', format: 'escpos', dots: 384 }),
    frame({ id: 2, op: 'raster', template: '<b>Label</b>', format: 'zpl', dots: 384 }),
  ]));
  const byId = Object.fromEntries(frames.map((f) => [f.header.id, f]));
  assert.deepEqual([...byId[1].body.subarray(0, 2)], [0x1b, 0x40], 'ESC @ initialises the printer');
  assert.match(byId[2].body.toString('latin1'), /\^XA/);
});

test('cli: `printers` prints one line of JSON and exits 0', () => {
  const r = spawnSync(binary, ['printers'], { encoding: 'utf8', timeout: 30_000 });
  assert.equal(r.status, 0);
  const out = JSON.parse(r.stdout.trim().split('\n').pop());
  assert.equal(out.ok, true);
  assert.ok(Array.isArray(out.printers));
});

test('cli: `print` to an unreachable printer prints {ok:false,error} and exits 1', () => {
  const r = spawnSync(binary, ['print', '--json', JSON.stringify({ printerId: 'net://127.0.0.1:1', template: '<b>x</b>' })], { encoding: 'utf8', timeout: 30_000 });
  assert.equal(r.status, 1);
  const out = JSON.parse(r.stdout.trim().split('\n').pop());
  assert.equal(out.ok, false);
  assert.equal(out.error.code, 'PRINTER_UNREACHABLE');
});

test('cli: `print` reads its request from standard input and from a file, and rejects bad JSON', () => {
  const fromStdin = spawnSync(binary, ['print', '--request', '-'], { input: JSON.stringify({ printerId: 'lpt1', template: 'x' }), encoding: 'utf8', timeout: 30_000 });
  assert.equal(fromStdin.status, 1);
  assert.equal(JSON.parse(fromStdin.stdout.trim().split('\n').pop()).error.code, 'INVALID_FIELD');
  const bad = spawnSync(binary, ['print', '--json', '{not json'], { encoding: 'utf8', timeout: 30_000 });
  assert.equal(bad.status, 1);
  assert.equal(JSON.parse(bad.stdout.trim().split('\n').pop()).ok, false);
  const missing = spawnSync(binary, ['print', '--request', '/no/such/request.json'], { encoding: 'utf8', timeout: 30_000 });
  assert.equal(missing.status, 1);
});

test('cli: `healthcheck` exits non-zero when no server answers, and an unknown command exits 64 without starting a server', () => {
  const hc = spawnSync(binary, ['healthcheck'], { encoding: 'utf8', timeout: 30_000, env: { ...process.env, API_PORT: '1' } });
  assert.equal(hc.status, 1);
  const unknown = spawnSync(binary, ['no-such-command'], { encoding: 'utf8', timeout: 30_000 });
  assert.equal(unknown.status, 64);
  assert.match(unknown.stderr, /unknown command/);
});
