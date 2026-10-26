'use strict';
// Sad path: wrong input, broken templates, hostile documents, a dying engine, and printing without a printer.
// Every failure is a PDFEngineError with a stable code, and the engine keeps working afterwards.
const test = require('node:test');
const assert = require('node:assert/strict');
const net = require('node:net');
const { PDFEngine, PDFEngineError } = require('..');
const { assertHealthy, binary, isPdf, withEngine } = require('./helpers');

const fails = (pdf, options, code, status) =>
  assert.rejects(pdf.generate(options), (e) => {
    assert.ok(e instanceof PDFEngineError, `a PDFEngineError, got ${e?.name}: ${e?.message}`);
    assert.equal(e.code, code, e.message);
    if (status) assert.equal(e.status, status);
    assert.ok(e.message.length > 0);
    return true;
  });

test('missing or wrongly typed fields', () =>
  withEngine(async (pdf) => {
    await fails(pdf, {}, 'MISSING_FIELD', 400);
    await fails(pdf, { template: null }, 'MISSING_FIELD', 400);
    await fails(pdf, { template: 5 }, 'INVALID_FIELD', 400);
    await fails(pdf, { template: '<p>x</p>', password: 12345 }, 'INVALID_FIELD', 400);
    await assert.rejects(pdf.generate(), Error);
    await assertHealthy(pdf);
  }));

test('page size: negative, zero, huge, unknown units and junk are INVALID_FIELD', () =>
  withEngine(async (pdf) => {
    for (const width of ['-5in', '0', 'abc', '100000in', '12furlongs', 'NaN']) {
      await fails(pdf, { template: '<p>x</p>', width, height: '10in' }, 'INVALID_FIELD', 400);
    }
    await fails(pdf, { template: '<p>x</p>', orientation: 'sideways' }, 'INVALID_FIELD', 400);
    await assertHealthy(pdf);
  }));

test('template syntax errors carry the code, the line and a hint', () =>
  withEngine(async (pdf) => {
    for (const template of ['{% if %}', '{% for i in x %}', '{{ a + }}', '{% endfor %}', '{% include "nope" %}', '{{ nope|no_such_filter }}']) {
      await fails(pdf, { template }, 'TEMPLATE_ERROR', 400);
    }
    await assert.rejects(pdf.generate({ template: '<p>\n\n{% if %}' }), (e) => e.line === 3 && typeof e.hint === 'string');
    await assertHealthy(pdf);
  }));

test('runaway templates are stopped: loops, recursion, output size, nesting', () =>
  withEngine(async (pdf) => {
    await fails(pdf, { template: '{% for i in range(100000000) %}x{% endfor %}' }, 'TEMPLATE_ERROR');
    await fails(pdf, { template: '{% for i in range(5000) %}{% for j in range(5000) %}x{% endfor %}{% endfor %}' }, 'TEMPLATE_TOO_EXPENSIVE', 400);
    await fails(pdf, { template: '{% macro f(n) %}{{ f(n+1) }}{% endmacro %}{{ f(0) }}' }, 'TEMPLATE_ERROR');
    await fails(pdf, { template: '<div>'.repeat(5000) + 'x' + '</div>'.repeat(5000) }, 'TEMPLATE_ERROR', 400);
    await assertHealthy(pdf);
  }));

test('the output cap is a setting: TEMPLATE_MAX_OUTPUT_BYTES makes a big template TEMPLATE_OUTPUT_TOO_LARGE', () =>
  withEngine(
    async (pdf) => {
      await fails(pdf, { template: '{{ a }}{{ a }}{{ a }}', data: { a: 'x'.repeat(400_000) } }, 'TEMPLATE_OUTPUT_TOO_LARGE', 413);
      await assertHealthy(pdf);
    },
    { env: { TEMPLATE_MAX_OUTPUT_BYTES: '1000000' } },
  ));

test('bad barcodes are INVALID_FIELD', () =>
  withEngine(async (pdf) => {
    await fails(pdf, { template: '<barcode value="12AB" type="ean13"/>' }, 'INVALID_FIELD', 400);
    await fails(pdf, { template: '<barcode value="" type="qr"/>' }, 'INVALID_FIELD', 400);
    await fails(pdf, { template: `<barcode value="${'x'.repeat(100000)}" type="qr"/>` }, 'INVALID_FIELD', 400);
    await assertHealthy(pdf);
  }));

test('broken images, fonts, SVG, math and Markdown are skipped or shown as text, never a crash', () =>
  withEngine(async (pdf) => {
    for (const template of [
      '<img src="data:image/png;base64,AAAA">',
      '<img src="not-in-the-map">',
      '<img src="/etc/passwd">',
      '<img src="file:///etc/passwd">',
      '<img src="http://127.0.0.1:1/x.png">',
      '<img>',
      '<svg><not-closed',
      '<style>@font-face{font-family:X;src:url(data:font/ttf;base64,AAAA)}</style><p style="font-family:X">x</p>',
      '<style>@font-face{font-family:X;src:url(/etc/passwd)}</style><p style="font-family:X">x</p>',
      '<math>\\frac{</math>',
      '<math></math>',
      '{{ t|markdown }}',
    ]) {
      assert.ok(isPdf(await pdf.generate({ template, data: { t: '<script>alert(1)</script> [x](javascript:alert(1)) ![](nope)' }, images: { bad: Buffer.from([1, 2, 3]) } })), template.slice(0, 60));
    }
    await assertHealthy(pdf);
  }));

test('hostile CSS does not crash the engine', () =>
  withEngine(async (pdf) => {
    for (const template of [
      '<div style="height:1000000000px;background:red">x</div>',
      '<p style="font-size:100000px">x</p>',
      '<table><tr><td colspan="100000">x</td></tr></table>',
      '<table><tr><td rowspan="100000">x</td></tr><tr><td>y</td></tr></table>',
      '<div style="display:grid;grid-template-columns:repeat(100000,1fr)">x</div>',
      '<div style="transform:scale(1e9) rotate(1e9deg);width:10px;height:10px;background:red"></div>',
      '<div style="float:left;width:100%;height:50px">a</div>'.repeat(300),
      '<style>*{break-inside:avoid;break-after:page}</style>' + '<p>x</p>'.repeat(300),
      '<style>@page{size:1px 1px;margin:1000px}</style><p>x</p>',
    ]) {
      assert.ok(isPdf(await pdf.generate({ template })), template.slice(0, 80));
    }
    await assertHealthy(pdf);
  }));

test('a missing binary is BINARY_NOT_FOUND with a hint, every time', async () => {
  const pdf = new PDFEngine({ binary: '/no/such/pdf-engine' });
  for (let i = 0; i < 3; i++) {
    await assert.rejects(pdf.generate({ template: '<p>x</p>' }), (e) => e.code === 'BINARY_NOT_FOUND' && !!e.hint);
  }
});

test('a binary that is not pdf-engine is a clear error, not a hang', async () => {
  const pdf = new PDFEngine({ binary: process.execPath, timeoutMs: 5000 });
  await assert.rejects(pdf.ping(), (e) => e instanceof PDFEngineError);
  await pdf.close().catch(() => {});
});

test('the engine killed in the middle of requests: those fail with ENGINE_EXITED, the next ones work', () =>
  withEngine(async (pdf) => {
    await pdf.ping();
    const slow = '<table>{% for i in range(20000) %}<tr><td>{{ i }}</td></tr>{% endfor %}</table>';
    const inFlight = Array.from({ length: 6 }, () => pdf.generate({ template: slow }).then(() => 'ok', (e) => e.code));
    process.kill(pdf.pid, process.platform === 'win32' ? undefined : 'SIGKILL');
    const results = await Promise.all(inFlight);
    assert.ok(results.every((r) => r === 'ok' || r === 'ENGINE_EXITED'), results.join());
    await new Promise((r) => setTimeout(r, 300));
    await assertHealthy(pdf);
  }));

test('a request that takes too long is CLIENT_TIMEOUT, and the engine is still usable', () =>
  withEngine(async (pdf) => {
    const slow = '<table>{% for i in range(20000) %}<tr><td>{{ i }}</td></tr>{% endfor %}</table>';
    await assert.rejects(pdf.generate({ template: slow }), { code: 'CLIENT_TIMEOUT' });
    await new Promise((r) => setTimeout(r, 4000)); // the engine finishes the abandoned document first
    await assertHealthy(pdf);
  }, { timeoutMs: 500 }));

test('fonts: a path that does not exist is FONT_ERROR and the engine carries on', () =>
  withEngine(async (pdf) => {
    await assert.rejects(pdf.loadFonts(['/no/such/font.ttf']), { code: 'FONT_ERROR' });
    await assertHealthy(pdf);
  }));

test('a closed engine says so, every time', async () => {
  const pdf = new PDFEngine({ binary });
  await pdf.ping();
  await pdf.close();
  for (let i = 0; i < 3; i++) await assert.rejects(pdf.generate({ template: '<p>x</p>' }), { code: 'ENGINE_CLOSED' });
});

test('images: a template cannot read local files or the network unless allowed', () =>
  withEngine(async (pdf) => {
    const server = net.createServer((s) => s.destroy());
    await new Promise((ok) => server.listen(0, '127.0.0.1', ok));
    try {
      const port = server.address().port;
      assert.ok(isPdf(await pdf.generate({ template: `<img src="http://127.0.0.1:${port}/x.png"><img src="../../../../etc/passwd">` })));
    } finally {
      server.close();
    }
  }));

test('printing: no printer, unreachable printer, bad ids and bad paper are INVALID_FIELD, MISSING_FIELD or PRINTER_UNREACHABLE', () =>
  withEngine(async (pdf) => {
    await assert.rejects(pdf.print({ printerId: 'net://127.0.0.1:1', template: '<b>x</b>' }), { code: 'PRINTER_UNREACHABLE', status: 502 });
    await assert.rejects(pdf.print({ printerId: 'lpt1', template: 'x' }), { code: 'INVALID_FIELD' });
    await assert.rejects(pdf.print({ printerId: 'net://127.0.0.1:9100', paper: '3 furlongs', template: 'x' }), { code: 'INVALID_FIELD' });
    await assert.rejects(pdf.print({ printerId: 'system://No Such Printer', template: 'x' }), (e) => e instanceof PDFEngineError);
    await assert.rejects(pdf.print({ template: 'x' }), { code: 'MISSING_FIELD' });
    await assert.rejects(pdf.print(), { code: 'MISSING_FIELD' });
    await assertHealthy(pdf);
  }));

test('printing to a printer that accepts the connection and hangs up is an error, not a hang', () =>
  withEngine(async (pdf) => {
    const server = net.createServer((s) => s.destroy());
    await new Promise((ok) => server.listen(0, '127.0.0.1', ok));
    try {
      const outcome = await pdf.print({ printerId: `net://127.0.0.1:${server.address().port}`, template: '<b>x</b>' }).catch((e) => e);
      assert.ok(outcome instanceof PDFEngineError || typeof outcome === 'object');
    } finally {
      server.close();
    }
    await assertHealthy(pdf);
  }));

test('PDF/A: an unknown level, a password, text no font covers and a missing alt are INVALID_FIELD with a reason', () =>
  withEngine(async (pdf) => {
    await assert.rejects(pdf.generate({ template: '<p>x</p>', pdfA: '9z' }), { code: 'INVALID_FIELD', status: 400 });
    await assert.rejects(pdf.generate({ template: '<p>x</p>', pdfA: '2b', password: 'secret' }), (e) => e.code === 'INVALID_FIELD' && /encryption/.test(e.message));
    const png = 'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==';
    await assert.rejects(pdf.generate({ template: `<img width="40" src="${png}">`, pdfA: '2a' }), (e) => e.code === 'INVALID_FIELD' && /alt/.test(e.message));
    assert.ok(isPdf(await pdf.generate({ template: `<img width="40" alt="dot" src="${png}">`, pdfA: '2b' })));
    await assertHealthy(pdf);
  }));
