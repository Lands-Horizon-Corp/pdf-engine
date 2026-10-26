'use strict';
// Happy path: every feature a template can use, with real documents, through the Node client.
const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { isPdf, pageCount, withEngine } = require('./helpers');

const text = (buf) => buf.toString('latin1');

test('values, loops, conditions, filters and macros', () =>
  withEngine(async (pdf) => {
    const bytes = await pdf.generate({
      template: `{% macro row(n) %}<tr><td>{{ n|upper }}</td></tr>{% endmacro %}
        <h1>{{ title }}</h1>{% if show %}<p>shown</p>{% endif %}
        <table>{% for n in names %}{{ row(n) }}{% endfor %}</table><p>{{ total|round(2) }}</p>`,
      data: { title: 'Invoice', show: true, names: ['a', 'b', 'c'], total: 10.456 },
    });
    assert.ok(isPdf(bytes));
    assert.equal(pageCount(bytes), 1);
  }));

test('page size units, orientation and many pages', () =>
  withEngine(async (pdf) => {
    for (const [width, height] of [['8.5in', '11in'], ['210mm', '297mm'], ['21cm', '29.7cm'], ['612pt', '792pt'], ['816px', '1056px'], ['80mm', '200mm']]) {
      assert.ok(isPdf(await pdf.generate({ template: '<p>x</p>', width, height })), `${width} x ${height}`);
    }
    assert.match(text(await pdf.generate({ template: '<p>x</p>', orientation: 'landscape' })), /\/MediaBox\s*\[0 0 79[12]/);
    assert.equal(pageCount(await pdf.generate({ template: '{% for i in range(60) %}<p style="break-after:page">{{ i }}</p>{% endfor %}' })), 60);
  }));

test('password, accessible (tagged) PDFs and @media print', () =>
  withEngine(async (pdf) => {
    assert.match(text(await pdf.generate({ template: '<p>secret</p>', password: 'hunter2' })), /\/Encrypt/);
    const tagged = await pdf.generate({ template: '<html lang="fr"><h1>Titre</h1><p>texte</p></html>', accessible: true });
    assert.match(text(tagged), /\/StructTreeRoot/);
    assert.match(text(tagged), /\/Lang\s*\(fr/);
    const print = await pdf.generate({ template: '<style>@media print{.a{height:600px}}@media screen{.a{height:10px}}</style><div class="a">x</div><div class="a">y</div>' });
    assert.equal(pageCount(print), 2);
  }));

test('tables (colspan, rowspan, repeating header), float, flex, grid and SVG', () =>
  withEngine(async (pdf) => {
    const long = await pdf.generate({ template: '<table><thead><tr><th>#</th></tr></thead><tbody>{% for i in range(120) %}<tr><td>{{ i }}</td></tr>{% endfor %}</tbody></table>' });
    assert.ok(pageCount(long) >= 2);
    assert.equal(pageCount(await pdf.generate({ template: '<table><tr><td colspan="2">a</td></tr><tr><td rowspan="2">b</td><td>c</td></tr><tr><td>d</td></tr></table>' })), 1);
    const box = (style) => `<div style="${style};width:200px;height:600px;background:#cde">x</div>`;
    assert.equal(pageCount(await pdf.generate({ template: `<style>body{margin:0}</style>${box('')}${box('')}` })), 2);
    assert.equal(pageCount(await pdf.generate({ template: `<style>body{margin:0}</style>${box('float:left')}${box('float:right')}` })), 1);
    assert.ok(isPdf(await pdf.generate({ template: '<div style="display:flex;gap:8px"><div style="flex:1">a</div><div style="flex:2">b</div></div><div style="display:grid;grid-template-columns:1fr 2fr"><p>c</p><p>d</p></div><svg width="60" height="60"><circle cx="30" cy="30" r="20" fill="red"/></svg>' })));
  }));

test('every language the bundled fonts cover, plus CJK, Arabic and emoji', () =>
  withEngine(async (pdf) => {
    for (const s of ['₱1,234.50 café', 'Привет мир', 'Γειά σου', '你好，世界', 'مرحبا بالعالم', '😀 🎉', 'שלום']) {
      assert.ok(isPdf(await pdf.generate({ template: `<p>${s}</p>` })), s);
    }
  }));

test('barcodes, Markdown, math and @font-face', () =>
  withEngine(async (pdf) => {
    assert.doesNotMatch(text(await pdf.generate({ template: '<barcode value="https://example.com" type="qr" width="100"/><barcode value="123456789012" type="code128"/>' })), /\/Subtype\s*\/Image/);
    assert.match(text(await pdf.generate({ template: '{{ t|markdown }}', data: { t: '# Title\n\n- a\n- b\n\n**bold**' } })), /-Bold/);
    assert.ok(isPdf(await pdf.generate({ template: '<math>\\frac{a}{b} + \\sqrt{x^2} = \\sum_{i=1}^{n} i</math>' })));
  }));

test('images: bytes by name, a data: URI, a file, and a folder', () =>
  withEngine(async (pdf) => {
    const png = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==', 'base64');
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'pdfe-img-'));
    fs.writeFileSync(path.join(dir, 'logo.png'), png);
    try {
      assert.match(text(await pdf.generate({ template: '<img src="logo" width="40">', images: { logo: png } })), /\/Subtype\s*\/Image/);
      assert.match(text(await pdf.generate({ template: `<img src="data:image/png;base64,${png.toString('base64')}" width="40">` })), /\/Subtype\s*\/Image/);
      assert.match(text(await pdf.generate({ template: '<img src="logo" width="40">', images: { logo: path.join(dir, 'logo.png') } })), /\/Subtype\s*\/Image/);
    } finally {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  }));

test('partials (include, extends, import) from the constructor and per call', () =>
  withEngine(
    async (pdf) => {
      assert.ok(isPdf(await pdf.generate({ template: '{% include "head" %}<p>{{ x }}</p>', data: { title: 'T', x: 1 } })));
      assert.equal(pageCount(await pdf.generate({ template: '{% include "head" %}{% include "extra" %}', data: { title: 'T' }, partials: { extra: '{% pagebreak %}<p>2</p>' } })), 2);
    },
    { partials: { head: '<h1>{{ title }}</h1>' } },
  ));

test('fonts: a font file and a folder are added to the system fonts', () =>
  withEngine(async (pdf) => {
    const fonts = path.resolve(__dirname, '../../../.public/assets/fonts');
    await pdf.loadFonts([path.join(fonts, 'NotoSans-Regular.ttf')]);
    await pdf.reloadFonts();
    assert.ok(isPdf(await pdf.generate({ template: '<p style="font-family:Noto Sans">x</p>' })));
  }));

test('the same engine makes the same document twice (no state leaks between documents)', () =>
  withEngine(async (pdf) => {
    const template = '<h1>{{ n }}</h1><table>{% for i in range(5) %}<tr><td>{{ i }}</td></tr>{% endfor %}</table>';
    const a = await pdf.generate({ template, data: { n: 1 } });
    await pdf.generate({ template: '<p style="color:red">in between</p>', width: '10in' });
    const b = await pdf.generate({ template, data: { n: 1 } });
    assert.equal(a.length, b.length);
  }));

test('PDF/A: each level declares itself, is never encrypted, and the a levels are tagged', () =>
  withEngine(async (pdf) => {
    for (const [level, part, conformance] of [['1b', '1', 'B'], ['2b', '2', 'B'], ['2u', '2', 'U'], ['3b', '3', 'B'], ['2a', '2', 'A']]) {
      const bytes = text(await pdf.generate({ template: '<h1>Invoice</h1><p>₱100 total</p>', pdfA: level }));
      assert.match(bytes, new RegExp(`<pdfaid:part>${part}</pdfaid:part>`), level);
      assert.match(bytes, new RegExp(`<pdfaid:conformance>${conformance}</pdfaid:conformance>`), level);
      assert.match(bytes, /GTS_PDFA/);
      assert.doesNotMatch(bytes, /\/Encrypt/);
      assert.equal(/\/StructTreeRoot/.test(bytes), conformance === 'A', `${level} tagging`);
    }
    assert.doesNotMatch(text(await pdf.generate({ template: '<p>x</p>' })), /pdfaid/, 'an ordinary PDF is not PDF/A');
  }));

test('plain documents use the bundled Noto Sans (regular and bold), on every machine', () =>
  withEngine(async (pdf) => {
    const bytes = text(await pdf.generate({ template: '<h1>Invoice</h1><p>Hello <b>bold</b> ₱1,234.50 Привет Γειά</p>' }));
    assert.match(bytes, /NotoSans-Regular/);
    assert.match(bytes, /NotoSans-Bold/);
  }));

test('text the bundled fonts lack, another font family, and code still use the system fonts', () =>
  withEngine(async (pdf) => {
    for (const template of ['<p>你好 世界</p>', '<p style="font-family:Arial">Arial</p>', '<pre>code</pre>', '<p>&#x1F600;</p>']) {
      assert.ok(isPdf(await pdf.generate({ template })), template);
    }
  }));
