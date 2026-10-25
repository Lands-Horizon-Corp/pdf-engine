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
`range(3)`, `namespace()` for counters, `{% include %}` is not available (there are no other templates to load).

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
| `<thead>` repeating on later pages | Split long tables per page, or repeat the header yourself |
| `background: url()`, `@font-face`, `@import` | `<img>`; the `fonts` option of the packages |
| `float`, `z-index`, `rowspan` | Flexbox or grid; reorder; nested tables |
| `@page { size }`, `@media print` | `width` / `height`; unconditional rules |
| `-webkit-background-clip: text` | A solid text color |
| Vertical padding on `display: inline-block` | `display: block` or flexbox |
| JavaScript in templates | Compute values before calling `generate()` |

Tips:

- Set `font-family: 'Noto Sans', sans-serif` for text that must look the same everywhere (the browser package ships
  Noto Sans; the server image ships about 150 Noto fonts). Other scripts: [Fonts and languages](fonts-and-languages.md).
- Use `px`, `pt` or `mm` for sizes. `12px` text is a good body size on Letter or A4.
- Keep tables `width: 100%` with `border-collapse: collapse` for clean ledgers and invoices.
- Page breaks: `page-break-before: always` starts a new page; `page-break-inside: avoid` keeps a block together.

## Images

`<img src="...">` accepts PNG, JPEG, GIF and SVG. An image with no size, or only a width or height, keeps its
proportions.

| `src` | Where it works |
| --- | --- |
| `data:image/png;base64,...` | Everywhere, with nothing else needed |
| A name you map: `<img src="logo">` | Every package: pass `images: { logo: bytesOrFileOrUrl }` |
| `https://cdn.example.com/logo.png` | Browser package (the browser fetches it; the host must allow CORS). Server and Electron: allow the host first (`ASSET_ALLOWED_HOSTS`, `imageHosts`) |
| `logo.png` or `file:///...` | Electron/Node: files inside `imageDir`. Server: `ASSET_LOCAL_DIR`. Never from a web page |

An image that cannot be loaded is skipped (the browser package tells `onImageError`); the document still renders.
Limits: 5 MB per image, 20 remote images per document on the server (`ASSET_MAX_BYTES`, `ASSET_MAX_IMAGES`), and
200 per document in the browser.
