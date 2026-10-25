import test from 'node:test';
import assert from 'node:assert/strict';
import http from 'node:http';
import { PDFEngine } from '../index.js';

const latin1 = (bytes) => Buffer.from(bytes).toString('latin1');
const isPdf = (bytes) => latin1(bytes.subarray(0, 5)) === '%PDF-';

/** A pdf-engine look-alike: answers every render with `%PDF-fake <name>` and remembers what it was sent. */
async function fakeServer(name, { status = 200 } = {}) {
  const seen = [];
  const server = http.createServer((req, res) => {
    const chunks = [];
    req.on('data', (c) => chunks.push(c));
    req.on('end', () => {
      seen.push({ auth: req.headers.authorization, body: Buffer.concat(chunks).toString('utf8') });
      if (status !== 200) return res.writeHead(status, { 'content-type': 'application/json' }).end(JSON.stringify({ error: 'busy', code: 'INTERNAL_ERROR', status }));
      res.writeHead(200, { 'content-type': 'application/pdf' }).end(`%PDF-fake ${name}`);
    });
  });
  await new Promise((ok) => server.listen(0, '127.0.0.1', ok));
  return { url: `http://127.0.0.1:${server.address().port}`, seen, close: () => new Promise((ok) => server.close(ok)) };
}

test('several servers: round robin, each with its own token', async () => {
  const [a, b] = [await fakeServer('a'), await fakeServer('b')];
  const pdf = new PDFEngine({ url: [a.url, b.url], token: ['token-a', 'token-b'], wasm: '/unused.wasm' });
  try {
    const names = [];
    for (let i = 0; i < 4; i++) names.push(latin1(await pdf.generate({ template: '<p>x</p>' })).slice(10));
    assert.deepEqual(names, ['a', 'b', 'a', 'b']);
    assert.deepEqual(a.seen.map((s) => s.auth), ['Bearer token-a', 'Bearer token-a']);
    assert.deepEqual(b.seen.map((s) => s.auth), ['Bearer token-b', 'Bearer token-b']);
  } finally {
    await a.close();
    await b.close();
  }
});

test('a failing server is skipped; client errors are not retried', async () => {
  const [bad, good] = [await fakeServer('bad', { status: 503 }), await fakeServer('good')];
  const pdf = new PDFEngine({ url: [bad.url, good.url] });
  try {
    for (let i = 0; i < 3; i++) assert.equal(latin1(await pdf.generate({ template: '<p>x</p>' })), '%PDF-fake good');
    assert.equal(bad.seen.length, 1, 'tried once, then left alone for a while');
  } finally {
    await bad.close();
    await good.close();
  }
  const rejecting = await fakeServer('r', { status: 422 });
  const other = await fakeServer('o');
  try {
    await assert.rejects(new PDFEngine({ url: [rejecting.url, other.url] }).generate({ template: 'x' }), { status: 422 });
    assert.equal(other.seen.length, 0);
  } finally {
    await rejecting.close();
    await other.close();
  }
});

test('pick auto: small documents render in the page, big ones go to the server', async () => {
  const server = await fakeServer('s');
  const pdf = new PDFEngine({ url: server.url });
  try {
    const small = await pdf.generate({ template: '<p>{{ a }}</p>', data: { a: 1 }, pick: 'auto', autoMaxBytes: '1kb' });
    assert.ok(isPdf(small) && !latin1(small).includes('fake'), 'rendered by WebAssembly');
    assert.equal(server.seen.length, 0);
    const big = await pdf.generate({ template: '<p>{{ a }}</p>', data: { a: 'x'.repeat(5000) }, pick: 'auto', autoMaxBytes: '1kb' });
    assert.equal(latin1(big), '%PDF-fake s');
    // pick: 'browser' never calls the server.
    assert.ok(isPdf(await pdf.generate({ template: '<p>x</p>', pick: 'browser' })));
    assert.equal(server.seen.length, 1);
  } finally {
    pdf.close();
    await server.close();
  }
});

test('pick auto falls back to WebAssembly when the servers are down; pick server does not', async () => {
  const down = ['http://127.0.0.1:1', 'http://127.0.0.1:2'];
  const pdf = new PDFEngine({ url: down });
  try {
    const bytes = await pdf.generate({ template: '<p>offline</p>', pick: 'auto' });
    assert.ok(isPdf(bytes) && !latin1(bytes).includes('fake'));
    await assert.rejects(pdf.generate({ template: '<p>x</p>', pick: 'server' }), { code: 'SERVER_UNREACHABLE' });
    // Opt in to the fallback with an explicit server pick.
    assert.ok(isPdf(await pdf.generate({ template: '<p>x</p>', pick: 'server', fallback: true })));
  } finally {
    pdf.close();
  }
});

test('data the template never names is not sent', async () => {
  const server = await fakeServer('s');
  const pdf = new PDFEngine({ url: server.url });
  try {
    await pdf.generate({ template: '<p>{{ name }} {{ order.total }}</p>', data: { name: 'A', order: { total: 5 }, secret: 'hunter2', huge: 'x'.repeat(1000) } });
    const body = server.seen[0].body;
    assert.match(body, /"name":"A"/);
    assert.match(body, /"order":\{"total":5\}/);
    assert.doesNotMatch(body, /hunter2|huge/);
    // Switched off, everything is sent.
    await pdf.generate({ template: '<p>{{ name }}</p>', data: { name: 'A', extra: 'kept' }, pruneData: false });
    assert.match(server.seen[1].body, /kept/);
  } finally {
    await server.close();
  }
});
