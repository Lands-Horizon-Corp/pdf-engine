import test from 'node:test';
import assert from 'node:assert/strict';
import { ServerPool, choose, isRetryable, parseBytes, pruneData, requestBytes, serversOf } from '../routing.js';

test('parseBytes reads numbers and size strings', () => {
  assert.equal(parseBytes(1500), 1500);
  assert.equal(parseBytes('512kb'), 512 * 1024);
  assert.equal(parseBytes('2mb'), 2 * 1024 * 1024);
  assert.equal(parseBytes('1.5 GB'), 1.5 * 1024 ** 3);
  assert.equal(parseBytes('100'), 100);
  for (const bad of ['two', '-1', '2tb', '', null, NaN]) assert.throws(() => parseBytes(bad), RangeError, String(bad));
});

test('serversOf lines up urls and tokens', () => {
  assert.deepEqual(serversOf({}), []);
  assert.deepEqual(serversOf({ url: 'https://a/', token: 't' }), [{ url: 'https://a', token: 't' }]);
  assert.deepEqual(serversOf({ url: ['https://a', 'https://b'], token: 'same' }), [
    { url: 'https://a', token: 'same' },
    { url: 'https://b', token: 'same' },
  ]);
  assert.deepEqual(serversOf({ url: ['https://a', 'https://b'], token: ['one', 'two'] }), [
    { url: 'https://a', token: 'one' },
    { url: 'https://b', token: 'two' },
  ]);
  // A short token list leaves the rest without one; blank urls are dropped.
  assert.deepEqual(serversOf({ url: ['https://a', '', 'https://c'], token: ['x'] }), [
    { url: 'https://a', token: 'x' },
    { url: 'https://c', token: undefined },
  ]);
});

test('requestBytes counts the template and the data in bytes', () => {
  assert.equal(requestBytes({ template: 'abc' }), 3);
  assert.equal(requestBytes({ template: '₱', data: { a: 1 } }), 3 + '{"a":1}'.length);
  assert.equal(requestBytes({}), 0);
});

test('choose: pick, size and connectivity', () => {
  const size = (n) => () => n;
  const base = { hasServers: true, autoMaxBytes: '2mb' };
  assert.equal(choose({ ...base, pick: 'browser', size: size(1e9) }), 'browser');
  assert.equal(choose({ ...base, pick: 'server', size: size(1) }), 'server');
  assert.equal(choose({ ...base, pick: 'auto', size: size(1024) }), 'browser');
  assert.equal(choose({ ...base, pick: 'auto', size: size(3 * 1024 * 1024) }), 'server');
  assert.equal(choose({ ...base, pick: 'auto', size: size(2 * 1024 * 1024) }), 'browser', 'the limit itself stays local');
  // No limit given: auto prefers the server.
  assert.equal(choose({ hasServers: true, size: size(1) }), 'server');
  // Offline, or no servers: always the page.
  assert.equal(choose({ ...base, pick: 'server', online: false, size: size(1e9) }), 'browser');
  assert.equal(choose({ pick: 'server', hasServers: false, size: size(1e9) }), 'browser');
  assert.throws(() => choose({ ...base, pick: 'cloud', size: size(1) }), RangeError);
  // The size is not computed when it does not matter.
  const never = () => assert.fail('size was computed');
  assert.equal(choose({ ...base, pick: 'server', size: never }), 'server');
  assert.equal(choose({ ...base, pick: 'browser', size: never }), 'browser');
});

test('isRetryable: network and server errors, not client errors', () => {
  assert.equal(isRetryable({ code: 'SERVER_UNREACHABLE' }), true);
  for (const status of [429, 502, 503, 504]) assert.equal(isRetryable({ status }), true, String(status));
  for (const status of [400, 401, 413, 422, 500]) assert.equal(isRetryable({ status }), false, String(status));
  assert.equal(isRetryable(new Error('x')), false);
});

const servers = ['https://a', 'https://b', 'https://c'].map((url) => ({ url }));
const unreachable = () => Object.assign(new Error('down'), { code: 'SERVER_UNREACHABLE' });

test('ServerPool rotates through the servers', async () => {
  const pool = new ServerPool(servers);
  const used = [];
  for (let i = 0; i < 6; i++) await pool.run(async (s) => used.push(s.url));
  assert.deepEqual(used, ['https://a', 'https://b', 'https://c', 'https://a', 'https://b', 'https://c']);
});

test('ServerPool fails over and avoids a server that just failed', async () => {
  let time = 0;
  const pool = new ServerPool(servers, { cooldownMs: 1000, now: () => time });
  const calls = [];
  const run = async (s) => {
    calls.push(s.url);
    if (s.url === 'https://a') throw unreachable();
    return s.url;
  };
  assert.equal(await pool.run(run), 'https://b'); // a failed, b answered
  assert.deepEqual(calls, ['https://a', 'https://b']);
  calls.length = 0;
  await pool.run(run);
  await pool.run(run);
  assert.ok(!calls.includes('https://a'), `a is on cooldown: ${calls}`);
  // After the cooldown it is tried again.
  time = 5000;
  calls.length = 0;
  for (let i = 0; i < 3; i++) await pool.run(run);
  assert.ok(calls.includes('https://a'));
});

test('ServerPool: a client error is not retried, and all servers down throws the last error', async () => {
  const pool = new ServerPool(servers);
  let calls = 0;
  await assert.rejects(
    pool.run(async () => {
      calls++;
      throw Object.assign(new Error('bad template'), { status: 422 });
    }),
    /bad template/,
  );
  assert.equal(calls, 1);

  calls = 0;
  await assert.rejects(
    pool.run(async () => {
      calls++;
      throw unreachable();
    }),
    (e) => e.code === 'SERVER_UNREACHABLE',
  );
  assert.equal(calls, 3);
});

test('pruneData keeps only the keys the template names', () => {
  const data = { name: 'A', order: { total: 1 }, items: [], unused: 1, other: 2 };
  assert.deepEqual(pruneData('{{ name }} {{ order.total }}{% for i in items %}{{ i }}{% endfor %}', data), { name: 'A', order: { total: 1 }, items: [] });
  assert.deepEqual(pruneData('{# note: other #}{{ name | upper }}', data), { name: 'A', other: 2 }, 'a comment counts as use (kept to be safe)');
  assert.deepEqual(pruneData('plain text', data), {});
  assert.equal(pruneData('{{ name }}', { name: 'A' }).name, 'A');
  const same = { a: 1 };
  assert.equal(pruneData('{{ a }}', same), same, 'nothing to drop: the same object');
  // Dynamic lookups and non-objects are left alone.
  assert.equal(pruneData('{% include "x" %}', data), data);
  assert.equal(pruneData('{{ self }}', data), data);
  assert.deepEqual(pruneData('{{ x }}', [1, 2]), [1, 2]);
  assert.equal(pruneData('{{ x }}', undefined), undefined);
});
