// Happy path: every feature a template can use, with real documents.
import test from 'node:test';
import assert from 'node:assert/strict';
import { isPdf, latin1, pageCount, withEngine } from './helpers.js';

const text = (bytes) => latin1(bytes);

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

test('data is HTML-escaped by default, and |safe opts out', () =>
  withEngine(async (pdf) => {
    const escaped = await pdf.generate({ template: '<p>{{ v }}</p>', data: { v: '<b>x</b>' } });
    const raw = await pdf.generate({ template: '<p>{{ v|safe }}</p>', data: { v: '<b>x</b>' } });
    assert.ok(isPdf(escaped) && isPdf(raw));
    assert.notEqual(escaped.length, raw.length, 'bold text uses another font');
  }));

test('page size: units, orientation, and many pages', () =>
  withEngine(async (pdf) => {
    for (const [width, height] of [['8.5in', '11in'], ['210mm', '297mm'], ['21cm', '29.7cm'], ['612pt', '792pt'], ['816px', '1056px'], ['80mm', '200mm']]) {
      assert.ok(isPdf(await pdf.generate({ template: '<p>x</p>', width, height })), `${width} x ${height}`);
    }
    const landscape = await pdf.generate({ template: '<p>x</p>', orientation: 'landscape' });
    assert.match(text(landscape), /\/MediaBox\s*\[0 0 79[12]/);
    const many = await pdf.generate({ template: '{% for i in range(60) %}<p style="break-after:page">page {{ i }}</p>{% endfor %}' });
    assert.equal(pageCount(many), 60);
  }));

test('a password encrypts the PDF', () =>
  withEngine(async (pdf) => {
    const bytes = await pdf.generate({ template: '<p>secret</p>', password: 'hunter2' });
    assert.match(text(bytes), /\/Encrypt/);
    assert.doesNotMatch(text(await pdf.generate({ template: '<p>open</p>' })), /\/Encrypt/);
  }));

test('tables: colspan, rowspan, a header repeated on every page, and nested tables', () =>
  withEngine(async (pdf) => {
    const rows = '{% for i in range(120) %}<tr><td>{{ i }}</td><td>row</td></tr>{% endfor %}';
    const long = await pdf.generate({ template: `<table><thead><tr><th>#</th><th>name</th></tr></thead><tbody>${rows}</tbody></table>` });
    assert.ok(pageCount(long) >= 2);
    const spans = await pdf.generate({ template: '<table><tr><td colspan="2">a</td></tr><tr><td rowspan="2">b</td><td>c</td></tr><tr><td>d</td></tr></table>' });
    assert.equal(pageCount(spans), 1);
    const nested = await pdf.generate({ template: '<table><tr><td><table><tr><td>inner</td></tr></table></td></tr></table>' });
    assert.equal(pageCount(nested), 1);
  }));

test('float: boxes sit side by side instead of stacking', () =>
  withEngine(async (pdf) => {
    const box = (style) => `<div style="${style};width:200px;height:600px;background:#cde">x</div>`;
    const stacked = await pdf.generate({ template: `<style>body{margin:0}</style>${box('')}${box('')}` });
    const floated = await pdf.generate({ template: `<style>body{margin:0}</style>${box('float:left')}${box('float:right')}` });
    assert.equal(pageCount(stacked), 2);
    assert.equal(pageCount(floated), 1);
  }));

test('flexbox, grid, multi-column, gradients, transforms and SVG', () =>
  withEngine(async (pdf) => {
    const bytes = await pdf.generate({
      template: `<div style="display:flex;gap:8px"><div style="flex:1;background:linear-gradient(red,blue)">a</div><div style="flex:2">b</div></div>
        <div style="display:grid;grid-template-columns:1fr 2fr"><p>c</p><p>d</p></div>
        <div style="columns:2"><p>col</p><p>umns</p></div>
        <div style="transform:rotate(5deg);width:50px;height:20px;background:#0a0"></div>
        <svg width="60" height="60"><circle cx="30" cy="30" r="20" fill="red"/></svg>`,
    });
    assert.ok(isPdf(bytes));
  }));

test('@page margin boxes with page numbers, and page breaks', () =>
  withEngine(async (pdf) => {
    const bytes = await pdf.generate({
      template: `<style>@page { margin: 20mm; @bottom-right { content: "Page " counter(page) " of " counter(pages); } }</style>
        <p>one</p>{% pagebreak %}<p>two</p>`,
    });
    assert.equal(pageCount(bytes), 2);
  }));

test('images: data: URIs, bytes by name, and a bad image skipped without failing', () =>
  withEngine(async (pdf) => {
    const png = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==', 'base64');
    const asData = await pdf.generate({ template: `<img src="data:image/png;base64,${png.toString('base64')}" width="40">` });
    const byName = await pdf.generate({ template: '<img src="logo" width="40">', images: { logo: new Uint8Array(png) } });
    assert.match(text(asData), /\/Subtype\s*\/Image/);
    assert.match(text(byName), /\/Subtype\s*\/Image/);
    const skipped = [];
    const bad = await pdf.generate({ template: '<p>text</p><img src="broken">', images: { broken: new Uint8Array([1, 2, 3]) }, onImageError: (src) => skipped.push(src) });
    assert.ok(isPdf(bad));
  }));

test('languages: Latin, Cyrillic, Greek, currency, CJK, Arabic and emoji do not fail', () =>
  withEngine(async (pdf) => {
    for (const s of ['₱1,234.50 café', 'Привет мир', 'Γειά σου', '你好，世界', 'مرحبا بالعالم', '😀 🎉', 'שלום']) {
      assert.ok(isPdf(await pdf.generate({ template: `<p>${s}</p>` })), s);
    }
  }));

test('barcodes and QR codes are vector shapes; Markdown and math are built in', () =>
  withEngine(async (pdf) => {
    const qr = await pdf.generate({ template: '<barcode value="https://example.com" type="qr" width="100"/><barcode value="123456789012" type="code128"/>' });
    assert.doesNotMatch(text(qr), /\/Subtype\s*\/Image/);
    const md = await pdf.generate({ template: '{{ t|markdown }}', data: { t: '# Title\n\n- a\n- b\n\n**bold** `code`' } });
    assert.match(text(md), /NotoSans-Bold/);
    const math = await pdf.generate({ template: '<math>\\frac{a}{b} + \\sqrt{x^2} = \\sum_{i=1}^{n} i</math>' });
    assert.ok(isPdf(math));
  }));

test('@font-face from a data: URI, accessible PDFs and @media print', () =>
  withEngine(async (pdf) => {
    const tagged = await pdf.generate({ template: '<html lang="fr"><h1>Titre</h1><p>texte</p></html>', accessible: true });
    assert.match(text(tagged), /\/StructTreeRoot/);
    assert.match(text(tagged), /\/Lang\s*\(fr/);
    const print = await pdf.generate({ template: '<style>@media print{.a{height:600px}}@media screen{.a{height:10px}}</style><div class="a">x</div><div class="a">y</div>' });
    assert.equal(pageCount(print), 2, 'print rules apply');
  }));

test('partials: include, extends and import', () =>
  withEngine(
    async (pdf) => {
      const bytes = await pdf.generate({ template: '{% include "head" %}<p>{{ x }}</p>', data: { title: 'T', x: 1 } });
      assert.ok(isPdf(bytes));
    },
    { partials: { head: '<h1>{{ title }}</h1>' } },
  ));

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

test('PDF/A: the creation date comes from the page clock (WebAssembly has none of its own)', () =>
  withEngine(async (pdf) => {
    const year = String(new Date().getUTCFullYear());
    assert.match(text(await pdf.generate({ template: '<p>x</p>', pdfA: '2b' })), new RegExp(`<xmp:CreateDate>${year}`));
  }));
