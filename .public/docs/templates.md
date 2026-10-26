# Templates and CSS

A template is HTML with placeholders. The engine fills the placeholders from your data (a JSON object), turns
the HTML into a PDF, and sends back the bytes. The template language is
[minijinja](https://docs.rs/minijinja) (the Jinja family: `{{ }}` and `{% %}`).

> It is **not** Handlebars, Vue or JSX: `{{#each}}`, `v-for` and `{items.map(...)}` do nothing here.

## Values

```html
<h1>Invoice {{ no }}</h1>
<p>{{ customer.name }}, {{ customer.address.city }}</p>
<p>First item: {{ items[0].name }}</p>
```

- Values are **HTML-escaped**: data containing `<b>x</b>` prints as text. Use `{{ html|safe }}` only for HTML you
  trust.
- A missing value prints nothing. `{{ missing|default("-") }}` supplies a fallback.
- Numbers keep their JSON form: `1500` prints `1500`, `2.5` prints `2.5`.

## Loops and conditions

```html
{% for item in items %}
  <tr>
    <td>{{ loop.index }}</td>
    <td>{{ item.name }}</td>
    <td>₱{{ item.price }}</td>
  </tr>
{% else %}
  <tr><td colspan="3">No items</td></tr>
{% endfor %}

{% if total > 1000 %}Free delivery{% elif total > 0 %}Standard{% else %}Nothing to ship{% endif %}
```

Loop variables: `loop.index` (from 1), `loop.index0`, `loop.first`, `loop.last`, `loop.length`,
`loop.cycle("odd", "even")`. Other tools: `{% set total = 0 %}`, `{% macro row(x) %}...{% endmacro %}`,
`{% filter upper %}...{% endfilter %}`, `{% raw %}{{ shown as is }}{% endraw %}`, comments `{# like this #}`,
`range(3)`, `namespace()` for counters. `{% include %}`, `{% extends %}` and `{% import %}` load [partials](#partials-headers-footers-rows-and-layouts) you pass in.

A variable set inside a loop does not survive the loop (Jinja scoping). Keep a running total with `namespace`,
or better, add it up in your code and pass it as data:

```html
{% set ns = namespace(total=0) %}
{% for item in items %}{% set ns.total = ns.total + item.price %}{% endfor %}
Total: {{ ns.total }}
```

Expressions: `+ - * / // % **`, string join `~`, comparisons, `and` / `or` / `not`, `in`,
`"a" if cond else "b"`, tests such as `x is defined`, `n is even`, `n is divisibleby 3`.

## Filters

Use them with `|`: `{{ name|upper }}`.

| Group | Filters |
| --- | --- |
| Text | `upper`, `lower`, `title`, `capitalize`, `trim`, `replace("a","b")`, `indent`, `lines`, `escape`, `safe`, `string` |
| Numbers | `round(2)`, `abs`, `int`, `float`, `sum`, `min`, `max`, `bool` |
| Lists | `length`, `first`, `last`, `join(", ")`, `sort`, `reverse`, `unique`, `list`, `batch(3)`, `slice(3)`, `map(attribute="x")`, `selectattr("x")` |
| Objects | `items`, `dictsort`, `attr("x")` |
| Other | `default(value)`, `format(...)`, `pprint` |

`format` works like `printf`: `{{ "%.2f"|format(price) }}` prints `1500.50`, `{{ "%05d"|format(42) }}` prints `00042`.

**Not available:** `tojson`, `truncate`, `urlencode`, `now()` and any date helper, and thousands separators (there is
no `1,234,567` filter). Format those in your code and pass the result as data:

```js
data: { total: new Intl.NumberFormat('en-PH', { style: 'currency', currency: 'PHP' }).format(1234567.5) }   // ₱1,234,567.50
```

## Partials: headers, footers, rows and layouts

Split a template into named pieces and reuse them. Give `partials` (a map of name to template) to the constructor, to
one call, or as the `partials` form field on the server:

```ts
const pdf = new PDFEngine({
  partials: {
    header: '<div class="brand"><h1>{{ store }}</h1><hr></div>',
    row: '<tr><td>{{ item.name }}</td><td class="r">₱{{ item.price }}</td></tr>',
    layout: '<header>{% block head %}{% endblock %}</header>{% block body %}{% endblock %}<footer>Thank you</footer>',
  },
});

await pdf.generate({
  template: `{% include "header" %}
             <table>{% for item in items %}{% include "row" %}{% endfor %}</table>`,
  data: { store: 'Manila Central', items },
});

await pdf.generate({                       // a page layout that pages extend
  template: '{% extends "layout" %}{% block head %}{{ title }}{% endblock %}{% block body %}<p>...</p>{% endblock %}',
  data,
});
```

- `{% include "name" %}` pastes a partial and gives it the same data (inside a loop it sees the loop variable).
- `{% extends "name" %}` with `{% block %}` makes a layout that documents fill in; `{% import "name" as x %}` brings in
  macros (`{% macro row(x) %}...{% endmacro %}`) to call as `{{ x.row(item) }}`.
- Per-call partials are merged over the constructor's, and are not kept for the next call.
- A name is letters, digits, `_`, `-`, `.` and `/` (up to 64 characters); up to 100 partials and 5 MB in total. A syntax
  error in a partial is a `TEMPLATE_ERROR` that names it. Including a name that was not given is a `TEMPLATE_ERROR`.
- Because a partial can use any `data` key, templates that include or extend skip the browser package's
  `pruneData` (they keep the whole object).

## Page numbers, headers and footers

Margin boxes repeat on every page, outside the content. Set the page margin and put text in the boxes with `@page`:

```html
<style>
  @page {
    margin: 20mm;
    @top-center    { content: "{{ company }}"; font-size: 10px; color: #555; }
    @bottom-left   { content: "Confidential"; font-size: 10px; }
    @bottom-right  { content: "Page " counter(page) " of " counter(pages); font-size: 10px; }
  }
</style>
```

Positions: `@top-left`, `@top-center`, `@top-right`, `@bottom-left`, `@bottom-center`, `@bottom-right` (and the left and
right sides and corners). `counter(page)` is the page number and `counter(pages)` the total; the text can also come from a
`position: running(name)` element (`content: element(name)`) for a header with markup. Without a `@page` rule the page
has no margin, so the content fills it.

## Page breaks

`{% pagebreak %}` starts a new page:

```html
<h1>Invoice</h1> ... {% pagebreak %} <h1>Terms and conditions</h1> ...
```

It works in partials too. (It becomes an empty block with `page-break-after: always`, so you can write that yourself.)

## Barcodes and QR codes

`<barcode>` draws a vector QR code or barcode. It is drawn as shapes, so it stays sharp at any size and scans from
paper or screen. It runs after the template, so values work:

```html
<barcode value="https://example.com/pay/{{ invoice.no }}" type="qr" width="120" />
<barcode value="{{ invoice.no }}" type="code128" width="220" height="60" />
<barcode value="750103131130" type="ean13" />
```

| Attribute | Meaning |
| --- | --- |
| `value` | What to encode (required) |
| `type` | `qr` (default), `code128`, `code39`, `ean13`, `ean8` |
| `width`, `height` | Size in pixels. A QR code is square (give one); a barcode is 60 px high by default |
| `color`, `background` | CSS colours (default black, no background) |
| `margin` | Quiet zone in modules (4 for QR, 10 for barcodes; scanners need one) |
| `ec` | QR error correction: `L`, `M` (default), `Q` or `H` |
| `class`, `style` | Copied to the drawing |

`code128` packs digit-only values in pairs automatically and takes printable ASCII; `ean13` takes 12 or 13 digits (it
adds the check digit); `ean8` 7 or 8; `code39` capitals, digits and `- . $ / + % space`. A value that does not fit the
type is an `INVALID_FIELD` error that says why, rather than a barcode that will not scan. QR codes hold Unicode
(`₱`, `你好`). The tests render a QR code and decode it back from the PDF.

## Markdown

Write the text of a document in Markdown instead of HTML, in the template or in the data:

```html
{% filter markdown %}
## Release notes

This is **Markdown**, with a [link](https://example.com), `code`, lists and tables:

| Item | Qty |
|------|----:|
| Hosting | 1 |
{% endfilter %}

{{ report.summary|markdown }}
```

- **What it reads:** CommonMark (headings, emphasis, lists, links, images, quotes, code blocks) plus tables,
  `~~strikethrough~~` and task lists (`- [x] done`). Style the result with your own CSS: the output is plain HTML
  (`h1`, `p`, `ul`, `table`, `blockquote`, `pre`, ...).
- **Values are text:** raw HTML inside the Markdown is shown as text, not read as markup, and a `{{ }}` inside a value is
  not evaluated. So `{{ user_text|markdown }}` is safe for text you do not control.
- **Links and images:** only `http`, `https`, `mailto`, `tel`, relative and `#anchor` links are kept; `javascript:` and
  other schemes are removed. An image address follows the usual image rules (a `data:` URI always; a URL only from an
  allowed host).
- **Limit:** 2 MB of Markdown per use; more is cut.

## Math

Write a formula in LaTeX, between `<math>` tags, or from a value with the `math` filter. The formula is laid out with
the document's own text engine, as real text (vector, selectable), not as a picture.

```html
<p>Energy is <math>E = mc^2</math>, and the roots are</p>
<math display>x = \frac{-b \pm \sqrt{b^2 - 4ac}}{2a}</math>

{{ formula|math }}                      <!-- inline, from the data -->
{{ formula|math(display=true) }}        <!-- on a line of its own -->
```

`<math>` sits in the text; `<math display>` is centred on its own line and sets big operators with their limits above
and below. Inside the tag, write `<` as `&lt;` (a template value already is).

- **Reads:** letters, numbers, `+ - = < >` and the usual symbols, Greek (`\alpha` ... `\Omega`), `\frac`, `\binom`,
  `\sqrt` and `\sqrt[n]`, `^` and `_` (also both together, and `'` for a prime), `\sum \prod \int \iint \oint \lim`
  and friends with limits, `\left ... \right`, matrices (`pmatrix`, `bmatrix`, `vmatrix`, `matrix`), `cases`,
  `\text{...}`, `\mathbf`, `\mathbb` (ℝ ℕ ℤ ...), `\mathcal`, `\hat \bar \vec \tilde \dot`, `\overline`, `\underline`,
  `\boxed`, the spacing commands (`\, \; \quad \qquad`), and about 150 symbols: relations, arrows, set and logic
  symbols (`\leq \neq \approx \in \subset \forall \exists \Rightarrow \to \infty \partial \nabla ...`).
- **Fonts:** the symbols come from Noto Sans Math, which is built into the engine (cut down to the symbols it uses,
  about 50 KB), so a formula looks the same in the browser, on the server and in Electron. Letters use Noto Sans.
- **What it is not:** this is not TeX. Spacing and the height of brackets, roots and big operators are approximations;
  a formula with very tall parts can look looser than in a TeX document. No `\newcommand`, no `align` environments
  with several alignment points, no `\color`. A command it does not know is shown as written (`\foo`), never an error.
- **Limits:** a formula of up to 20,000 characters and 24 levels of nesting.

## Archival PDF/A

Set `pdfA` (the `pdf_a` form field on the server) for a file meant to be kept for decades: fonts are embedded, colours are
declared, the metadata says which standard it follows, and the file is **checked against that standard while it is made**.

```ts
await pdf.generate({ template, data, pdfA: '2b' });
```

| Level | Use it for |
| --- | --- |
| `2b` | The usual choice: archival, visually reproducible |
| `2u` | Also guarantees the text can be searched and copied (Unicode) |
| `3b` | Like 2b, and allows attached files |
| `1b` | The oldest standard, for systems that ask for it |
| `2a`, `3a` | Also tagged (a structure tree, like `accessible: true`); every `<img>` needs an `alt` |

- **Refused, not faked:** a document that cannot meet the standard is `INVALID_FIELD` with the reason, for example text in a
  script no font covers (add one with `@font-face`) or an image without `alt` at level `a`. You never get a file that claims
  PDF/A and does not conform.
- **No password:** PDF/A forbids encryption, so `pdfA` with `password` is `INVALID_FIELD`.
- **Images** are drawn without smoothing (PDF/A forbids the flag), so a small image scaled up looks blockier than in a
  normal PDF.
- **Date:** the creation date is the current time (the page's clock in the browser).
- **What is not checked:** the validator is the one built into the PDF writer (Krilla); we have not run an external one
  such as veraPDF on the output. If you need a certificate, validate your own documents with veraPDF.

## Accessible PDFs

Set `accessible: true` (the `accessible` form field on the server) for a **tagged PDF**: besides the drawing, the file
carries a structure tree (headings, paragraphs, lists, tables, links, images with their `alt` text), the document
language and the title, so a screen reader can read it in order.

```ts
await pdf.generate({ template, data, accessible: true });
```

```html
<html lang="fil"><head><title>Invoice 42</title></head><body><h1>Invoice</h1> ...</body></html>
```

- **Language:** the `lang` of `<html>`; English when there is none. **Title:** the `<title>`; "Document" when there is none.
- **Write real structure:** `<h1>`..`<h6>` for headings, `<table>` with `<th>` for data tables, `<ul>`/`<ol>` for lists,
  `alt` on every meaningful image. Text in `<div>`s and `<span>`s is read, but without headings and lists there is
  little for a reader to navigate.
- **PDF/UA:** a document with headings is declared PDF/UA (the standard needs an outline, which comes from the
  headings). A page with no headings is still tagged, with its language and title, but is not declared PDF/UA.
- **What is not checked:** the file is built to the structure the standard asks for, but we have not run an external
  conformance checker (such as veraPDF) or a screen-reader test suite on it. If you need a certificate, validate your
  own documents with one.
- **Cost:** a little extra time and file size for the structure; leave it off for receipts and labels.

## Repeating table headers

A table with a `<thead>` shows its header rows again at the top of every page the table continues onto, like a browser
printing a table. Put the header in `<thead>` (not in the first row of `<tbody>`):

```html
<table>
  <thead><tr><th>#</th><th>Item</th><th>Amount</th></tr></thead>
  <tbody> ...hundreds of rows... </tbody>
</table>
```

- Several header rows repeat together. A table that fits on one page is not changed.
- A table with a repeated header takes about one extra layout pass; a document without a `<thead>` pays nothing.
- A `<tfoot>` is not repeated.

## Guard rails

A template can only produce so much: output is capped (`TEMPLATE_MAX_OUTPUT_BYTES`) and execution has a budget
(`TEMPLATE_FUEL`), so a runaway loop fails with `TEMPLATE_TOO_EXPENSIVE` or `TEMPLATE_OUTPUT_TOO_LARGE` instead of
hanging. Syntax errors are `TEMPLATE_ERROR` with the line number.

The browser package sends only the `data` keys the template names (`pruneData`, on by default), so a large object
you do not use costs nothing.

## Page size and orientation

Every call takes `width`, `height` and `orientation`:

| Option | Values | Default |
| --- | --- | --- |
| `width`, `height` | a number with a unit: `in`, `mm`, `cm`, `pt`, `pc`, `px` (`8.5in`, `210mm`, `21cm`, `612pt`, `816px`) | `8.5in` x `11in` (US Letter) |
| `orientation` | `portrait`, `landscape` (landscape swaps width and height) | `portrait` |

Common sizes: A4 `210mm` x `297mm`, Letter `8.5in` x `11in`, a receipt `80mm` x `200mm`.
The page has no margin of its own: add `padding` to `body`.

## CSS

It supports a large part of modern CSS: flexbox, grid, tables, `border-radius`, gradients (linear and radial),
transforms, opacity, shadows, SVG, CSS variables, `calc()`, `@page` margin boxes and running headers,
`page-break-before/after/inside`, multi-column layout, counters, `::before` / `::after`, Unicode and
right-to-left text. Fixtures for each are in `tests/fixtures/css/`.

```html
<style>
  body { font-family: 'Noto Sans', sans-serif; font-size: 12px; padding: 32px; }
  table { width: 100%; border-collapse: collapse; }
  td, th { padding: 4px 8px; border-bottom: 1px solid #ddd; }
  tr { page-break-inside: avoid; }          /* never split a row across pages */
  .total { text-align: right; font-weight: 700; }
</style>
```

Known gaps and what to do instead:

| Not supported | Use instead |
| --- | --- |
| `background: url()`, `@import` | `<img>`; fonts: `@font-face` is supported ([Fonts](fonts-and-languages.md#font-face-a-font-a-document-brings-along)) |
| `float` (text wrapping under it), `z-index` | `float: left/right` is approximated, see below; otherwise flexbox or grid |
| `@page { size }` | `width` / `height` |
| `outline`, `filter`, `position: sticky` | A border or background instead |
| `-webkit-background-clip: text` | A solid text color |
| An inline `<svg>` wider than the space it sits in (even by 1 px) is not drawn at all | Give it `style="display:block;max-width:100%"`, or make it narrower. Text inside an SVG (`<text>`) may not show: put labels in HTML |
| An `<img>` with a percentage CSS width and no height (`width: 100%`) is drawn at size zero | Give it a `width="520"` attribute, or a width and a height |
| `<img>` or `background` that points at an SVG file | PNG, or paste the `<svg>` into the template |
| Vertical padding on `display: inline-block` | `display: block` or flexbox |
| JavaScript in templates | Compute values before calling `generate()` |

Tips:

- Set `font-family: 'Noto Sans', sans-serif` for text that must look the same everywhere (the browser package ships
  Noto Sans; the server image ships about 150 Noto fonts). Other scripts: [Fonts and languages](fonts-and-languages.md).
- Use `px`, `pt` or `mm` for sizes. `12px` text is a good body size on Letter or A4.
- Keep tables `width: 100%` with `border-collapse: collapse` for clean ledgers and invoices.
- Page breaks: `page-break-before: always` starts a new page; `page-break-inside: avoid` keeps a block together.

## Images

`<img src="...">` accepts PNG, JPEG and GIF. An image with no size, or only a width or height, keeps its
proportions. An inline `<svg>` element is drawn as vector shapes; an `<img>` or CSS background that points at an SVG
file or `data:image/svg+xml` is **not drawn yet** (it is skipped without an error), so use PNG for logos or paste the
`<svg>` into the template.

| `src` | Where it works |
| --- | --- |
| `data:image/png;base64,...` | Everywhere, with nothing else needed |
| A name you map: `<img src="logo">` | Every package: pass `images: { logo: bytesOrFileOrUrl }` |
| `https://cdn.example.com/logo.png` | Browser package (the browser fetches it; the host must allow CORS). Server and Electron: allow the host first (`ASSET_ALLOWED_HOSTS`, `imageHosts`) |
| `logo.png` or `file:///...` | Electron/Node: files inside `imageDir`. Server: `ASSET_LOCAL_DIR`. Never from a web page |

An image that cannot be loaded is skipped (the browser package tells `onImageError`); the document still renders.
Limits: 5 MB per image, 20 remote images per document on the server (`ASSET_MAX_BYTES`, `ASSET_MAX_IMAGES`), and
200 per document in the browser.


## Float (approximation)

`float: left` and `float: right` put boxes **side by side**: the floats of a block and the content that follows them (up to an
element with `clear`) share one wrapping row: left floats, the content, right floats. Text does **not** wrap underneath a
float: it sits beside it, or below it when less than about 150px is left. That covers image-plus-text, two columns and a
right-aligned total. Real text wrapping is not built yet.
