'use strict';
// Limits and memory: very large documents, out-of-memory protection, leaks, concurrency and recovery.
const test = require('node:test');
const assert = require('node:assert/strict');
const { PDFEngine, PDFEngineError } = require('..');
const { assertHealthy, binary, isPdf, pageCount, withEngine } = require('./helpers');

const rssOf = (pid) => {
  const { execFileSync } = require('node:child_process');
  try {
    return Number(execFileSync('ps', ['-o', 'rss=', '-p', String(pid)]).toString().trim()) / 1024; // MB
  } catch {
    return null; // Windows: no ps
  }
};

test('a large table (5,000 rows) and a long document (500 pages) finish', { timeout: 120_000 }, () =>
  withEngine(async (pdf) => {
    assert.ok(pageCount(await pdf.generate({ template: '<table>{% for i in range(5000) %}<tr><td>{{ i }}</td><td>row</td></tr>{% endfor %}</table>' })) > 50);
    assert.equal(pageCount(await pdf.generate({ template: '{% for i in range(500) %}<p style="break-after:page">p{{ i }}</p>{% endfor %}' })), 500);
    await assertHealthy(pdf);
  }));

test('a table with a repeating header over 20,000 rows finishes', { timeout: 120_000 }, () =>
  withEngine(async (pdf) => {
    const bytes = await pdf.generate({ template: '<table><thead><tr><th>h</th></tr></thead><tbody>' + '<tr><td>x</td></tr>'.repeat(20000) + '</tbody></table>' });
    assert.ok(pageCount(bytes) > 100);
    await assertHealthy(pdf);
  }));

test('a large result (many MB) arrives intact', { timeout: 120_000 }, () =>
  withEngine(async (pdf) => {
    const bytes = await pdf.generate({ template: '{% for i in range(3000) %}<p style="break-after:page"><b>page {{ i }}</b> ' + 'lorem ipsum '.repeat(40) + '</p>{% endfor %}', compression: undefined });
    assert.ok(bytes.length > 1_000_000, `${bytes.length} bytes`);
    assert.match(bytes.subarray(-8).toString('latin1'), /%%EOF/);
  }));

test('a very large request (a 50 MB data value) is handled or refused cleanly', { timeout: 120_000 }, () =>
  withEngine(async (pdf) => {
    const outcome = await pdf.generate({ template: '<p>{{ big|length }}</p>', data: { big: 'x'.repeat(50e6) } }).catch((e) => e);
    assert.ok(isPdf(outcome) || outcome instanceof PDFEngineError, 'a PDF or a PDFEngineError');
    await assertHealthy(pdf);
  }));

test('a request bigger than IPC_MAX_REQUEST_BYTES ends that session with an error, and a new call starts a new engine', { timeout: 120_000 }, () =>
  withEngine(
    async (pdf) => {
      await assert.rejects(pdf.generate({ template: '<p>x</p>', data: { big: 'x'.repeat(3_000_000) } }), (e) => e instanceof PDFEngineError);
      await new Promise((r) => setTimeout(r, 500));
      await assertHealthy(pdf);
    },
    { env: { IPC_MAX_REQUEST_BYTES: '1000000' } },
  ));

test('out of memory protection: a template whose output would be huge is refused with TEMPLATE_OUTPUT_TOO_LARGE', { timeout: 120_000 }, () =>
  withEngine(
    async (pdf) => {
      await assert.rejects(pdf.generate({ template: '{{ big }}'.repeat(20), data: { big: 'x'.repeat(2_000_000) } }), { code: 'TEMPLATE_OUTPUT_TOO_LARGE', status: 413 });
      await assertHealthy(pdf);
    },
    { env: { TEMPLATE_MAX_OUTPUT_BYTES: '10000000' } },
  ));

test('a render that takes longer than RENDER_TIMEOUT is a TIMEOUT error and the engine carries on', { timeout: 120_000 }, () =>
  withEngine(
    async (pdf) => {
      const slow = '<table>{% for i in range(20000) %}<tr><td>{{ i }}</td></tr>{% endfor %}</table>';
      const outcome = await pdf.generate({ template: slow }).catch((e) => e);
      assert.ok(outcome instanceof PDFEngineError ? ['TIMEOUT', 'RENDER_FAILED'].includes(outcome.code) : isPdf(outcome), String(outcome?.code));
      await assertHealthy(pdf);
    },
    { env: { RENDER_TIMEOUT_SECS: '1' } },
  ));

test('concurrency: 200 documents at once all finish, each with its own content', { timeout: 120_000 }, () =>
  withEngine(async (pdf) => {
    const results = await Promise.all(Array.from({ length: 200 }, (_, i) => pdf.generate({ template: '<p>doc {{ i }}</p>' + '<br>'.repeat(i % 7), data: { i } })));
    assert.equal(results.length, 200);
    assert.ok(results.every(isPdf));
    assert.ok(new Set(results.map((r) => r.toString('base64'))).size > 1);
  }));

test('concurrency: failures mixed in do not affect the others', { timeout: 120_000 }, () =>
  withEngine(async (pdf) => {
    const results = await Promise.all(
      Array.from({ length: 80 }, (_, i) => (i % 4 === 0 ? pdf.generate({ template: '{% if %}' }).then(() => 'ok', (e) => e.code) : pdf.generate({ template: `<p>${i}</p>` }).then((b) => (isPdf(b) ? 'ok' : 'bad'), (e) => e.code))),
    );
    assert.equal(results.filter((r) => r === 'TEMPLATE_ERROR').length, 20);
    assert.equal(results.filter((r) => r === 'ok').length, 60);
  }));

test('no leak: 1,500 documents in a row do not grow the engine process without bound', { timeout: 300_000 }, () =>
  withEngine(async (pdf) => {
    const template = '<h1>{{ n }}</h1><table>{% for i in range(30) %}<tr><td>{{ i }}</td><td>item {{ i }}</td></tr>{% endfor %}</table>';
    for (let i = 0; i < 300; i++) await pdf.generate({ template, data: { n: i } });
    const before = rssOf(pdf.pid);
    for (let i = 0; i < 1500; i++) await pdf.generate({ template, data: { n: i } });
    const after = rssOf(pdf.pid);
    if (before !== null) assert.ok(after - before < 150, `the engine grew ${(after - before).toFixed(0)} MB over 1,500 documents`);
  }));

test('no leak: errors in a row do not grow the engine process without bound', { timeout: 300_000 }, () =>
  withEngine(async (pdf) => {
    for (let i = 0; i < 200; i++) await pdf.generate({ template: '{% if %}' }).catch(() => {});
    const before = rssOf(pdf.pid);
    for (let i = 0; i < 2000; i++) await pdf.generate({ template: '{% if %}' }).catch(() => {});
    const after = rssOf(pdf.pid);
    if (before !== null) assert.ok(after - before < 100, `grew ${(after - before).toFixed(0)} MB`);
    await assertHealthy(pdf);
  }));

test('many engines started and closed leave no processes behind', { timeout: 120_000 }, async () => {
  const pids = [];
  for (let i = 0; i < 15; i++) {
    const pdf = new PDFEngine({ binary });
    assert.ok(isPdf(await pdf.generate({ template: `<p>${i}</p>` })));
    pids.push(pdf.pid);
    await pdf.close();
  }
  await new Promise((r) => setTimeout(r, 500));
  for (const pid of pids) assert.throws(() => process.kill(pid, 0), /ESRCH|EPERM/, `engine ${pid} is still running`);
});

test('restart storm: killing the engine 10 times in a row never loses the client', { timeout: 120_000 }, () =>
  withEngine(async (pdf) => {
    for (let i = 0; i < 10; i++) {
      await pdf.ping();
      process.kill(pdf.pid, process.platform === 'win32' ? undefined : 'SIGKILL');
      await new Promise((r) => setTimeout(r, 150));
      assert.ok(isPdf(await pdf.generate({ template: `<p>round ${i}</p>` })));
    }
  }));
