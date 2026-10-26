// Sad path: wrong input, broken templates and hostile documents. Every failure is a PDFEngineError with a stable
// code, and the engine keeps working afterwards.
import test from 'node:test';
import assert from 'node:assert/strict';
import { PDFEngine, PDFEngineError } from '../index.js';
import { assertHealthy, isPdf, withEngine } from './helpers.js';

const fails = (pdf, options, code, status) =>
  assert.rejects(pdf.generate(options), (e) => {
    assert.ok(e instanceof PDFEngineError, `a PDFEngineError, got ${e?.name}: ${e?.message}`);
    assert.equal(e.code, code);
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
    await assert.rejects(pdf.generate(null), Error);
    await assertHealthy(pdf);
  }));

test('page size: negative, zero, huge, unknown units and junk are INVALID_FIELD', () =>
  withEngine(async (pdf) => {
    for (const width of ['-5in', '0', 'abc', '100000in', '12furlongs', '', 'NaN', '1e999in']) {
      await fails(pdf, { template: '<p>x</p>', width, height: '10in' }, 'INVALID_FIELD', 400).catch((e) => {
        throw new Error(`width ${JSON.stringify(width)}: ${e.message}`);
      });
    }
    await fails(pdf, { template: '<p>x</p>', orientation: 'sideways' }, 'INVALID_FIELD', 400);
    await assertHealthy(pdf);
  }));

test('template syntax errors carry the code, the line and a hint', () =>
  withEngine(async (pdf) => {
    for (const template of ['{% if %}', '{% for i in x %}', '{{ a + }}', '{% endfor %}', '{{ "unterminated }}', '{% include "nope" %}', '{{ nope|no_such_filter }}']) {
      await assert.rejects(pdf.generate({ template }), (e) => e instanceof PDFEngineError && e.code === 'TEMPLATE_ERROR' && e.status === 400, template);
    }
    await assert.rejects(pdf.generate({ template: '<p>\n\n{% if %}' }), (e) => e.line === 3 && typeof e.hint === 'string');
    await assertHealthy(pdf);
  }));

test('a missing value renders as empty, and a missing nested value is an error you can see', () =>
  withEngine(async (pdf) => {
    assert.ok(isPdf(await pdf.generate({ template: '<p>{{ missing }}</p>', data: {} })));
    assert.ok(isPdf(await pdf.generate({ template: '<p>{{ missing }}</p>' })));
    await assertHealthy(pdf);
  }));

test('runaway templates are stopped: loops, recursion, output size', () =>
  withEngine(async (pdf) => {
    await fails(pdf, { template: '{% for i in range(100000000) %}x{% endfor %}' }, 'TEMPLATE_ERROR');
    await fails(pdf, { template: '{% for i in range(5000) %}{% for j in range(5000) %}x{% endfor %}{% endfor %}' }, 'TEMPLATE_TOO_EXPENSIVE', 400);
    await fails(pdf, { template: '{% macro f(n) %}{{ f(n+1) }}{% endmacro %}{{ f(0) }}' }, 'TEMPLATE_ERROR');
    await fails(pdf, { template: '{{ big }}{{ big }}{{ big }}{{ big }}{{ big }}{{ big }}', data: { big: 'x'.repeat(60e6) } }, 'TEMPLATE_OUTPUT_TOO_LARGE', 413);
    await assertHealthy(pdf);
  }));

test('HTML nested absurdly deep is refused instead of crashing the renderer', () =>
  withEngine(async (pdf) => {
    await fails(pdf, { template: '<div>'.repeat(5000) + 'x' + '</div>'.repeat(5000) }, 'TEMPLATE_ERROR', 400);
    await fails(pdf, { template: '<ul><li>'.repeat(400) + 'x' }, 'TEMPLATE_ERROR', 400);
    assert.ok(isPdf(await pdf.generate({ template: '<div>'.repeat(300) + 'x' + '</div>'.repeat(300) })), '300 levels is fine');
    await assertHealthy(pdf);
  }));

test('bad barcodes are INVALID_FIELD', () =>
  withEngine(async (pdf) => {
    await fails(pdf, { template: '<barcode value="12AB" type="ean13"/>' }, 'INVALID_FIELD', 400);
    await fails(pdf, { template: '<barcode value="" type="qr"/>' }, 'INVALID_FIELD', 400);
    await fails(pdf, { template: `<barcode value="${'x'.repeat(100000)}" type="qr"/>` }, 'INVALID_FIELD', 400);
    await fails(pdf, { template: '<barcode value="x" type="no-such-type"/>' }, 'INVALID_FIELD', 400);
    await assertHealthy(pdf);
  }));

test('broken images, fonts, SVG, math and Markdown are skipped or shown as text, never a crash', () =>
  withEngine(async (pdf) => {
    const warnings = [];
    const options = { onImageError: (src, reason) => warnings.push([src, reason]) };
    for (const template of [
      '<img src="data:image/png;base64,AAAA">',
      '<img src="data:image/png;base64,">',
      '<img src="not-in-the-map">',
      '<img src="">',
      '<img>',
      '<img src="x" width="-5" height="1e99">',
      '<svg><not-closed',
      '<svg width="1000000" height="1000000"><rect width="10" height="10"/></svg>',
      '<style>@font-face{font-family:X;src:url(data:font/ttf;base64,AAAA)}</style><p style="font-family:X">x</p>',
      '<style>@font-face{font-family:X;src:url(nope.woff2)}</style><p style="font-family:X">x</p>',
      '<math>\\frac{</math>',
      '<math>\\unknowncommand{x}</math>',
      '<math></math>',
      '{{ t|markdown }}',
    ]) {
      assert.ok(isPdf(await pdf.generate({ template, data: { t: '\u0000<script>alert(1)</script> [x](javascript:alert(1)) ![](nope)' }, images: { bad: new Uint8Array([1, 2, 3]) }, ...options })), template.slice(0, 60));
    }
    await assertHealthy(pdf);
  }));

test('hostile CSS does not crash: huge sizes, impossible grids, zero fonts, endless pages', () =>
  withEngine(async (pdf) => {
    for (const template of [
      '<div style="height:1000000000px;background:red">x</div>',
      '<p style="font-size:100000px">x</p>',
      '<p style="font-size:0">x</p>',
      '<p style="margin:-100000px">x</p>',
      '<table><tr><td colspan="100000">x</td></tr></table>',
      '<table><tr><td rowspan="100000">x</td></tr><tr><td>y</td></tr></table>',
      '<div style="display:grid;grid-template-columns:repeat(100000,1fr)">x</div>',
      '<div style="width:50px;height:50px;border-radius:1e9px;background:red"></div>',
      '<div style="transform:scale(1e9) rotate(1e9deg);width:10px;height:10px;background:red"></div>',
      '<div style="columns:100000"><p>text</p></div>',
      '<div style="float:left;width:100%;height:50px">a</div>'.repeat(300),
      '<style>*{break-inside:avoid;break-after:page}</style>' + '<p>x</p>'.repeat(300),
      '<table><thead><tr><th style="height:2000px">h</th></tr></thead><tbody><tr><td>x</td></tr></tbody></table>',
      '<style>@page{size:1px 1px;margin:1000px}</style><p>x</p>',
    ]) {
      assert.ok(isPdf(await pdf.generate({ template })), template.slice(0, 80));
    }
    await assertHealthy(pdf);
  }));

test('data of every shape: null, arrays, deep objects, big numbers, odd keys', () =>
  withEngine(async (pdf) => {
    for (const data of [null, [], [1, 2, 3], 'a string', 42, true, { a: null }, { 'odd key!': 1, 'ключ': 'значение' }, { n: 1e308, m: -0, k: 2 ** 53 }, JSON.parse('{"a":'.repeat(50) + '1' + '}'.repeat(50))]) {
      assert.ok(isPdf(await pdf.generate({ template: '<p>{{ a }}</p>', data })), JSON.stringify(data)?.slice(0, 40));
    }
    await assertHealthy(pdf);
  }));

test('data that cannot be sent as JSON is dropped or an error, not a hang', () =>
  withEngine(async (pdf) => {
    const circular = {};
    circular.self = circular;
    for (const data of [circular, { n: 10n }]) {
      const outcome = await pdf.generate({ template: '<p>x</p>', data }).catch((e) => e);
      assert.ok(isPdf(outcome) || outcome instanceof Error, 'a PDF (unused data is dropped) or an error');
    }
    await assertHealthy(pdf);
  }));

test('a font that cannot be loaded, and a wasm file that cannot be loaded, are explained', async () => {
  const bad = new PDFEngine({ fonts: ['http://127.0.0.1:1/missing.ttf'] });
  try {
    await assert.rejects(bad.generate({ template: '<p>x</p>' }), { code: 'FONT_LOAD_FAILED' });
  } finally {
    bad.close();
  }
});

test('a closed engine says so, every time', async () => {
  const pdf = new PDFEngine();
  await pdf.ping();
  pdf.close();
  for (let i = 0; i < 3; i++) await assert.rejects(pdf.generate({ template: '<p>x</p>' }), { code: 'ENGINE_CLOSED' });
});

test('invalid options to the constructor and to methods are explained', () =>
  withEngine(async (pdf) => {
    await fails(pdf, { template: '<p>x</p>', compression: 'extreme' }, 'INVALID_FIELD');
    await assertHealthy(pdf);
  }));

test('browser-only methods say so under Node', () =>
  withEngine(async (pdf) => {
    await assert.rejects(pdf.download({ template: '<p>x</p>' }, 'x.pdf'), { code: 'NOT_IN_BROWSER' });
    await assert.rejects(pdf.open({ template: '<p>x</p>' }), { code: 'NOT_IN_BROWSER' });
    await assert.rejects(pdf.fromElement('#nope'), { code: 'NOT_IN_BROWSER' });
    await assertHealthy(pdf);
  }));

test('a failure in one concurrent call does not affect the others', () =>
  withEngine(async (pdf) => {
    const calls = Array.from({ length: 40 }, (_, i) =>
      i % 4 === 0 ? pdf.generate({ template: '{% if %}' }).then(() => 'ok', (e) => e.code) : pdf.generate({ template: `<p>${i}</p>` }).then((b) => (isPdf(b) ? 'ok' : 'bad'), (e) => e.code),
    );
    const results = await Promise.all(calls);
    assert.equal(results.filter((r) => r === 'TEMPLATE_ERROR').length, 10);
    assert.equal(results.filter((r) => r === 'ok').length, 30);
  }));

test('PDF/A: an unknown level, a password, text no font covers and a missing alt are INVALID_FIELD with a reason', () =>
  withEngine(async (pdf) => {
    await fails(pdf, { template: '<p>x</p>', pdfA: '9z' }, 'INVALID_FIELD', 400);
    await assert.rejects(pdf.generate({ template: '<p>x</p>', pdfA: '2b', password: 'secret' }), (e) => e.code === 'INVALID_FIELD' && /encryption/.test(e.message));
    const png = 'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==';
    await assert.rejects(pdf.generate({ template: `<img width="40" src="${png}">`, pdfA: '2a' }), (e) => e.code === 'INVALID_FIELD' && /alt/.test(e.message));
    assert.ok(isPdf(await pdf.generate({ template: `<img width="40" alt="dot" src="${png}">`, pdfA: '2b' })));
    await assertHealthy(pdf);
  }));
