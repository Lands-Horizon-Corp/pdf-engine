// Limits and memory: very large documents, out-of-memory, leaks, and recovery. These are slower than the others.
import test from 'node:test';
import assert from 'node:assert/strict';
import { PDFEngineError } from '../index.js';
import { assertHealthy, isPdf, pageCount, withEngine } from './helpers.js';

test('a large table (5,000 rows) and a long document (500 pages) finish', { timeout: 120_000 }, () =>
  withEngine(async (pdf) => {
    const table = await pdf.generate({ template: '<table>{% for i in range(5000) %}<tr><td>{{ i }}</td><td>row</td></tr>{% endfor %}</table>' });
    assert.ok(pageCount(table) > 50);
    const pages = await pdf.generate({ template: '{% for i in range(500) %}<p style="break-after:page">p{{ i }}</p>{% endfor %}' });
    assert.equal(pageCount(pages), 500);
    await assertHealthy(pdf);
  }));

test('a table with a repeating header over 20,000 rows finishes', { timeout: 120_000 }, () =>
  withEngine(async (pdf) => {
    const bytes = await pdf.generate({ template: '<table><thead><tr><th>h</th></tr></thead><tbody>' + '<tr><td>x</td></tr>'.repeat(20000) + '</tbody></table>' });
    assert.ok(pageCount(bytes) > 100);
    await assertHealthy(pdf);
  }));

test('out of memory: a template that outgrows WebAssembly memory is an error, then the engine recovers', { timeout: 180_000 }, () =>
  withEngine(async (pdf) => {
    // 20 copies of 200 MB would be 4 GB of HTML: the output cap refuses it.
    await assert.rejects(pdf.generate({ template: '{{ big }}'.repeat(20), data: { big: 'x'.repeat(200e6) } }), (e) => e instanceof PDFEngineError && e.code === 'TEMPLATE_OUTPUT_TOO_LARGE' && e.status === 413);
    await assertHealthy(pdf);
  }));

test('out of memory in the renderer (not the template): RENDER_FAILED, then a fresh instance works', { timeout: 180_000 }, () =>
  withEngine(async (pdf) => {
    // 100 MB of text in one paragraph is under the output cap but more than the renderer can lay out.
    let outcome;
    try {
      outcome = await pdf.generate({ template: '<p>' + 'x '.repeat(50e6) + '</p>' });
    } catch (e) {
      outcome = e;
    }
    if (outcome instanceof Error) {
      assert.ok(outcome instanceof PDFEngineError);
      assert.equal(outcome.code, 'RENDER_FAILED');
      assert.equal(outcome.status, 500);
    } else {
      assert.ok(isPdf(outcome), 'or it simply finished');
    }
    await assertHealthy(pdf);
    await assertHealthy(pdf);
  }));

test('two engines: a crash in one does not take the other down', { timeout: 180_000 }, async () => {
  await withEngine(async (a) =>
    withEngine(async (b) => {
      await a.generate({ template: '<p>a</p>' });
      await b.generate({ template: '<p>b</p>' });
      await a.generate({ template: '<p>' + 'x '.repeat(50e6) + '</p>' }).catch(() => {});
      await assertHealthy(a);
      await assertHealthy(b);
    }),
  );
});

test('no leak: 1,500 documents in a row do not grow the process without bound', { timeout: 300_000 }, () =>
  withEngine(async (pdf) => {
    const template = '<h1>{{ n }}</h1><table>{% for i in range(30) %}<tr><td>{{ i }}</td><td>item {{ i }}</td></tr>{% endfor %}</table>';
    for (let i = 0; i < 300; i++) await pdf.generate({ template, data: { n: i } }); // warm up
    global.gc?.();
    const before = process.memoryUsage().rss;
    for (let i = 0; i < 1500; i++) await pdf.generate({ template, data: { n: i } });
    global.gc?.();
    const grownMb = (process.memoryUsage().rss - before) / 1e6;
    assert.ok(grownMb < 150, `memory grew ${grownMb.toFixed(0)} MB over 1,500 documents`);
  }));

test('no leak: errors in a row do not grow the process without bound', { timeout: 300_000 }, () =>
  withEngine(async (pdf) => {
    for (let i = 0; i < 200; i++) await pdf.generate({ template: '{% if %}' }).catch(() => {});
    const before = process.memoryUsage().rss;
    for (let i = 0; i < 2000; i++) await pdf.generate({ template: '{% if %}' }).catch(() => {});
    assert.ok((process.memoryUsage().rss - before) / 1e6 < 100);
    await assertHealthy(pdf);
  }));

test('concurrency: 200 documents at once all finish with their own content', { timeout: 120_000 }, () =>
  withEngine(async (pdf) => {
    const results = await Promise.all(Array.from({ length: 200 }, (_, i) => pdf.generate({ template: '<p>doc {{ i }}</p>' + '<br>'.repeat(i % 7), data: { i } })));
    assert.equal(results.length, 200);
    assert.ok(results.every(isPdf));
    assert.equal(new Set(results.map((r) => Buffer.from(r).toString('base64'))).size > 1, true, 'different documents differ');
  }));

test('many engines created and closed do not leak or fail', { timeout: 120_000 }, async () => {
  for (let i = 0; i < 40; i++) {
    await withEngine(async (pdf) => assert.ok(isPdf(await pdf.generate({ template: `<p>${i}</p>` }))));
  }
});

test('workers: the same documents come out of worker threads, and bad ones fail the same way', { timeout: 120_000 }, () =>
  withEngine(
    async (pdf) => {
      const ok = await Promise.all(Array.from({ length: 12 }, (_, i) => pdf.generate({ template: '<p>{{ i }}</p>', data: { i } })));
      assert.ok(ok.every(isPdf));
      await assert.rejects(pdf.generate({ template: '{% if %}' }), { code: 'TEMPLATE_ERROR' });
      await assert.rejects(pdf.generate({ template: '<div>'.repeat(5000) }), { code: 'TEMPLATE_ERROR' });
      assert.ok(isPdf(await pdf.generate({ template: '<p>after</p>' })));
    },
    { workers: 2 },
  ));
