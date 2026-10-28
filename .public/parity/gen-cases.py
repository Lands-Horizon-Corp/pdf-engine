#!/usr/bin/env python3
"""Writes the pagination parity cases into cases/<category>/*.html (run: python3 gen-cases.py).

The pages use fixed-height blocks and short one-line rows, so what is compared is where the page breaks fall, not how
two different fonts wrap a paragraph. Every case sets @page { margin: 0.5in } (Letter, so 960 px of room per page)."""
import base64, os, shutil, struct, zlib

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, 'cases')
CATEGORIES = ['page-count', 'forced-breaks', 'break-inside', 'table-pagination', 'image-boundaries', 'flex-grid-pages', 'fixed-header-footer', 'complex-tables']
for c in CATEGORIES:
    shutil.rmtree(os.path.join(OUT, c), ignore_errors=True)
    os.makedirs(os.path.join(OUT, c))

BASE = '''<!doctype html><html><head><meta charset="utf-8"><style>
@page{size:Letter;margin:0.5in}
*{box-sizing:border-box}
body{margin:0;font:14px Arial,Helvetica,sans-serif;color:#111}
.bar{height:30px;line-height:30px;padding:0 10px;border-bottom:1px solid #fff;color:#fff;font-size:12px}
.c0{background:#2563eb}.c1{background:#16a34a}.c2{background:#dc2626}.c3{background:#9333ea}.c4{background:#d97706}
%s
</style></head><body>%s</body></html>'''

def write(cat, name, css, body):
    with open(os.path.join(OUT, cat, name + '.html'), 'w') as f:
        f.write(BASE % (css, body))

def bars(n, h=30, label='row'):
    return ''.join(f'<div class="bar c{i % 5}" style="height:{h}px;line-height:{h}px">{label} {i + 1}</div>' for i in range(n))

def png(w, h):
    """A vertical rainbow so that a cut-off image shows."""
    rows = b''
    for y in range(h):
        t = y / max(h - 1, 1)
        rows += b'\x00' + bytes([int(255 * (1 - t)), int(255 * (1 - abs(2 * t - 1))), int(255 * t)]) * w
    def chunk(k, d):
        c = struct.pack('>I', len(d)) + k + d
        return c + struct.pack('>I', zlib.crc32(k + d) & 0xffffffff)
    return b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', w, h, 8, 2, 0, 0, 0)) + chunk(b'IDAT', zlib.compress(rows)) + chunk(b'IEND', b'')

IMG = 'data:image/png;base64,' + base64.b64encode(png(32, 64)).decode()
def img(w, h, extra=''):
    return f'<img src="{IMG}" style="display:block;width:{w}px;height:{h}px;{extra}">'

# ---- 1. page count and overflow ------------------------------------------------------------------------------------
c = 'page-count'
write(c, '01-six-pages', '', bars(190))
write(c, '02-ten-pages', '', bars(320))
write(c, '03-exactly-one-page-plus-one-row', '', bars(33))
write(c, '04-varying-heights', '', ''.join(f'<div class="bar c{i % 5}" style="height:{20 + (i * 7) % 41}px;line-height:{20 + (i * 7) % 41}px">block {i + 1}</div>' for i in range(150)))
write(c, '05-ordered-list-250', 'ol{margin:0;padding-left:30px}li{height:24px;line-height:24px}', '<ol>' + ''.join(f'<li>item {i + 1}</li>' for i in range(250)) + '</ol>')
write(c, '06-headings-and-lines', 'h2{margin:0;height:40px;line-height:40px;font-size:20px}p{margin:0;height:22px;line-height:22px}', ''.join(f'<h2>Section {i + 1}</h2>' + ''.join(f'<p>line {j + 1} of section {i + 1}</p>' for j in range(12)) for i in range(8)))
write(c, '07-twelve-pages-of-thin-rows', '', ''.join(f'<div class="bar c{i % 5}" style="height:10px;line-height:10px;font-size:8px">r{i + 1}</div>' for i in range(1150)))
write(c, '08-one-tall-block', '.tall{height:6000px;background:linear-gradient(#dbeafe,#1e3a8a)}', '<div class="tall"></div>')
write(c, '09-nested-containers', '.a{padding:10px;background:#f1f5f9}.b2{padding:10px;background:#e2e8f0}', '<div class="a"><div class="b2">' + bars(200) + '</div></div>')
write(c, '10-one-block-per-page', '.page{height:960px;border:2px solid #333;padding:10px}', ''.join(f'<div class="page">page block {i + 1}</div>' for i in range(6)))
write(c, '11-definition-list', 'dl{margin:0}dt{height:22px;line-height:22px;font-weight:bold}dd{height:22px;line-height:22px;margin:0 0 0 30px}', '<dl>' + ''.join(f'<dt>term {i + 1}</dt><dd>definition {i + 1}</dd>' for i in range(110)) + '</dl>')
write(c, '12-two-pages-then-short-tail', '', bars(64) + '<div class="bar c2" style="height:30px">tail</div>')

# ---- 2. forced page breaks -----------------------------------------------------------------------------------------
c = 'forced-breaks'
sec = lambda i, extra='': f'<section style="height:120px;background:#dbeafe;{extra}">section {i}</section>'
write(c, '01-page-break-before-always', '', ''.join(sec(i, 'page-break-before:always' if i > 1 else '') for i in range(1, 6)))
write(c, '02-page-break-after-always', '', ''.join(sec(i, 'page-break-after:always') for i in range(1, 6)))
write(c, '03-break-before-page', '', ''.join(sec(i, 'break-before:page' if i > 1 else '') for i in range(1, 6)))
write(c, '04-break-after-page', '', ''.join(sec(i, 'break-after:page') for i in range(1, 6)))
write(c, '05-consecutive-breaks', '', sec(1) + sec(2, 'break-before:page') + sec(3, 'break-before:page;margin-top:0') + sec(4, 'break-before:page'))
write(c, '06-break-before-first-element', '', sec(1, 'break-before:page') + sec(2))
write(c, '07-break-after-last-element', '', sec(1) + sec(2, 'break-after:page'))
write(c, '08-break-before-right', '', sec(1) + sec(2, 'break-before:right') + sec(3))
write(c, '09-break-inside-nested-div', '.w{border:2px solid #333;padding:8px}', '<div class="w">' + sec(1) + '<div class="w">' + sec(2, 'break-before:page') + sec(3) + '</div></div>' + sec(4))
write(c, '10-break-before-list-items', 'ul{margin:0;padding-left:30px}li{height:40px;line-height:40px}', '<ul>' + ''.join(f'<li style="break-before:{"page" if i % 3 == 0 and i else "auto"}">item {i + 1}</li>' for i in range(10)) + '</ul>')
write(c, '11-forced-break-mid-page', '', bars(10) + sec('A', 'break-before:page') + bars(10, label='after') )
write(c, '12-forced-breaks-and-overflow', '', sec(1) + bars(40) + sec(2, 'break-before:page') + bars(40) + sec(3, 'break-before:page'))

# ---- 3. break-inside: avoid ----------------------------------------------------------------------------------------
c = 'break-inside'
card = lambda i, h=300, extra='': f'<div class="card" style="height:{h}px;{extra}">card {i}</div>'
CARD = '.card{margin:0 0 12px;border:2px solid #1d4ed8;background:#eff6ff;padding:10px}'
write(c, '01-cards-avoid', CARD + '.card{break-inside:avoid}', ''.join(card(i) for i in range(1, 11)))
write(c, '02-cards-no-avoid-control', CARD, ''.join(card(i) for i in range(1, 11)))
write(c, '03-cards-legacy-page-break-inside', CARD + '.card{page-break-inside:avoid}', ''.join(card(i) for i in range(1, 11)))
write(c, '04-tall-cards-avoid', CARD + '.card{break-inside:avoid}', ''.join(card(i, 420) for i in range(1, 8)))
write(c, '05-block-taller-than-a-page', CARD + '.card{break-inside:avoid}', card(1, 200) + card(2, 1300) + card(3, 200))
write(c, '06-list-items-avoid', 'li{break-inside:avoid;height:130px;margin-bottom:8px;background:#fef3c7;list-style:none;padding:8px}ul{margin:0;padding:0}', '<ul>' + ''.join(f'<li>entry {i}</li>' for i in range(1, 16)) + '</ul>')
write(c, '07-figures-avoid', 'figure{margin:0 0 12px;break-inside:avoid}figure div{height:260px;background:#bbf7d0}figcaption{height:24px;line-height:24px;background:#166534;color:#fff;padding:0 8px}', ''.join(f'<figure><div></div><figcaption>figure {i}</figcaption></figure>' for i in range(1, 9)))
write(c, '08-nested-avoid', CARD + '.o{break-inside:avoid;border:2px solid #333;padding:6px;margin-bottom:10px}.card{break-inside:avoid;height:140px}', ''.join(f'<div class="o">{card(i, 140)}{card(i + 100, 140)}</div>' for i in range(1, 8)))
write(c, '09-heading-keeps-with-next', 'h2{margin:0;height:36px;line-height:36px;break-after:avoid;background:#fde68a}p{margin:0 0 6px;height:150px;background:#f3f4f6}', ''.join(f'<h2>Heading {i}</h2><p>paragraph {i}</p>' for i in range(1, 12)))
write(c, '10-inline-block-cards-avoid', '.ib{display:inline-block;vertical-align:top;width:230px;height:200px;margin:0 8px 8px 0;background:#e9d5ff;break-inside:avoid}', ''.join(f'<div class="ib">tile {i}</div>' for i in range(1, 31)))
write(c, '11-orphans-widows-lines', 'p{margin:0 0 10px;orphans:3;widows:3;line-height:20px;width:300px}', ''.join('<p>' + '<br>'.join(f'line {j + 1}' for j in range(13)) + '</p>' for _ in range(6)))
write(c, '12-avoid-with-forced-break', CARD + '.card{break-inside:avoid}', card(1) + card(2, 300, 'break-before:page') + card(3) + card(4) + card(5))

# ---- 4. table pagination and repeating headers ---------------------------------------------------------------------
c = 'table-pagination'
T = 'table{width:100%;border-collapse:collapse}th,td{border:1px solid #999;padding:0 8px;height:28px;text-align:left}thead th{background:#1e40af;color:#fff}'
rows = lambda n, cols=3, h=28: ''.join('<tr>' + ''.join(f'<td style="height:{h}px">r{i + 1}c{j + 1}</td>' for j in range(cols)) + '</tr>' for i in range(n))
head = lambda cols=3: '<thead><tr>' + ''.join(f'<th>column {j + 1}</th>' for j in range(cols)) + '</tr></thead>'
write(c, '01-thead-repeats-60-rows', T, f'<table>{head()}<tbody>{rows(60)}</tbody></table>')
write(c, '02-no-thead', T, f'<table><tbody>{rows(60)}</tbody></table>')
write(c, '03-thead-and-tfoot', T + 'tfoot td{background:#e5e7eb;font-weight:bold}', f'<table>{head()}<tbody>{rows(60)}</tbody><tfoot><tr><td>total</td><td>x</td><td>y</td></tr></tfoot></table>')
write(c, '04-variable-row-heights', T, f'<table>{head()}<tbody>' + ''.join(f'<tr><td style="height:{24 + (i * 13) % 50}px">r{i + 1}</td><td>b</td><td>c</td></tr>' for i in range(45)) + '</tbody></table>')
write(c, '05-colspan-rows', T, f'<table>{head()}<tbody>' + ''.join('<tr><td colspan="3" style="background:#fef9c3">group</td></tr>' + rows(3) if i % 4 == 0 else rows(1) for i in range(36)) + '</tbody></table>')
write(c, '06-rowspan-cells', T, f'<table>{head()}<tbody>' + ''.join(f'<tr><td rowspan="2" style="height:56px">span {i}</td><td>a</td><td>b</td></tr><tr><td>c</td><td>d</td></tr>' for i in range(20)) + '</tbody></table>')
write(c, '07-rows-avoid-break', T + 'tr{break-inside:avoid}', f'<table>{head()}<tbody>{rows(20, h=90)}</tbody></table>')
write(c, '08-caption-and-header', T + 'caption{height:30px;font-weight:bold;text-align:left}', f'<table><caption>Quarterly figures</caption>{head()}<tbody>{rows(60)}</tbody></table>')
write(c, '09-fixed-layout-wide', T + 'table{table-layout:fixed}', f'<table>{head(6)}<tbody>{rows(55, 6)}</tbody></table>')
write(c, '10-two-tables-in-a-row', T + 'table{margin-bottom:24px}', f'<table>{head()}<tbody>{rows(40)}</tbody></table><table>{head()}<tbody>{rows(40)}</tbody></table>')
write(c, '11-table-in-padded-div', T + '.p{padding:20px;background:#f8fafc;border:1px solid #cbd5e1}', f'<div class="p"><table>{head()}<tbody>{rows(60)}</tbody></table></div>')
write(c, '12-zebra-and-empty-cells', T + 'tbody tr:nth-child(even){background:#f1f5f9}', f'<table>{head()}<tbody>' + ''.join(f'<tr><td>{i + 1}</td><td></td><td>{"" if i % 3 else "x"}</td></tr>' for i in range(70)) + '</tbody></table>')

# ---- 5. images near page boundaries --------------------------------------------------------------------------------
c = 'image-boundaries'
write(c, '01-image-straddles-break', '', bars(28) + img(600, 400) + bars(5))
write(c, '02-image-just-fits', '', bars(20) + img(600, 360) + bars(5))
write(c, '03-full-page-image', '', img(740, 940))
write(c, '04-two-large-images', '', img(700, 500) + img(700, 500) + img(700, 500))
write(c, '05-image-after-long-text', '', bars(30) + img(400, 300))
write(c, '06-image-with-margins', '', bars(25) + img(500, 300, 'margin:40px 0') + bars(4))
write(c, '07-figure-avoid-break', 'figure{margin:0;break-inside:avoid}', bars(26) + '<figure>' + img(600, 300) + '<figcaption class="bar c2">caption</figcaption></figure>')
write(c, '08-images-in-a-row', '.r{display:flex;gap:10px}', bars(24) + '<div class="r">' + img(220, 300) + img(220, 300) + img(220, 300) + '</div>')
write(c, '09-inline-images-in-text', 'p{margin:0;line-height:40px}img{display:inline-block !important;vertical-align:middle}', ''.join(f'<p>text {i} ' + img(40, 30, 'display:inline-block') + ' more</p>' for i in range(30)))
write(c, '10-ten-images-stacked', '', ''.join(img(600, 180, 'margin-bottom:10px') for _ in range(10)))
write(c, '11-image-taller-than-remaining-space', '', bars(10) + img(500, 800) + bars(3))
write(c, '12-background-image-block', '.bg{height:420px;background:url(' + IMG + ') center/cover}', bars(26) + '<div class="bg"></div>' + bars(3))

# ---- 6. flexbox and grid across pages ------------------------------------------------------------------------------
c = 'flex-grid-pages'
item = lambda i, h=60, extra='': f'<div class="it c{i % 5}" style="height:{h}px;{extra}">item {i}</div>'
IT = '.it{color:#fff;padding:6px;font-size:12px}'
write(c, '01-flex-column-many-items', IT + '.f{display:flex;flex-direction:column;gap:6px}', '<div class="f">' + ''.join(item(i) for i in range(1, 41)) + '</div>')
write(c, '02-flex-row-wrap', IT + '.f{display:flex;flex-wrap:wrap;gap:8px}.it{width:170px}', '<div class="f">' + ''.join(item(i, 150) for i in range(1, 61)) + '</div>')
write(c, '03-grid-three-columns', IT + '.g{display:grid;grid-template-columns:repeat(3,1fr);gap:8px}', '<div class="g">' + ''.join(item(i, 120) for i in range(1, 46)) + '</div>')
write(c, '04-grid-spans', IT + '.g{display:grid;grid-template-columns:repeat(4,1fr);gap:8px}.w2{grid-column:span 2}', '<div class="g">' + ''.join(item(i, 100, '' ) .replace('class="it', 'class="it w2' if i % 3 == 0 else 'class="it') for i in range(1, 41)) + '</div>')
write(c, '05-grid-auto-rows', IT + '.g{display:grid;grid-template-columns:1fr 1fr;grid-auto-rows:90px;gap:8px}', '<div class="g">' + ''.join(f'<div class="it c{i % 5}">cell {i}</div>' for i in range(1, 51)) + '</div>')
write(c, '06-flex-items-avoid-break', IT + '.f{display:flex;flex-wrap:wrap;gap:8px}.it{width:230px;break-inside:avoid}', '<div class="f">' + ''.join(item(i, 220) for i in range(1, 25)) + '</div>')
write(c, '07-nested-flex-in-grid', IT + '.g{display:grid;grid-template-columns:1fr 1fr;gap:10px}.f{display:flex;flex-direction:column;gap:4px;border:2px solid #333;padding:4px}', '<div class="g">' + ''.join('<div class="f">' + item(i, 70) + item(i + 100, 70) + '</div>' for i in range(1, 17)) + '</div>')
write(c, '08-flex-space-between-rows', '.r{display:flex;justify-content:space-between;height:44px;align-items:center;border-bottom:1px solid #ccc}', ''.join(f'<div class="r"><span>left {i}</span><span>middle</span><span>right {i}</span></div>' for i in range(1, 90)))
write(c, '09-grid-template-areas', IT + '.g{display:grid;grid-template-columns:200px 1fr;grid-template-areas:"s m";gap:8px}.s{grid-area:s;background:#475569;height:1400px}.m{grid-area:m;background:#e2e8f0;height:1400px}', '<div class="g"><div class="s it">sidebar</div><div class="m">main</div></div>')
write(c, '10-flex-grow-column-tall', '.f{display:flex;flex-direction:column;height:2000px;border:2px solid #333}.a{flex:1;background:#bfdbfe}.b3{flex:2;background:#bbf7d0}', '<div class="f"><div class="a">grow 1</div><div class="b3">grow 2</div></div>')
write(c, '11-grid-gap-large', IT + '.g{display:grid;grid-template-columns:repeat(2,1fr);gap:40px}', '<div class="g">' + ''.join(item(i, 150) for i in range(1, 21)) + '</div>')
write(c, '12-flex-wrap-mixed-heights', IT + '.f{display:flex;flex-wrap:wrap;align-items:flex-start;gap:8px}.it{width:150px}', '<div class="f">' + ''.join(item(i, 60 + (i * 37) % 160) for i in range(1, 51)) + '</div>')

# ---- 7. fixed and absolute headers/footers -------------------------------------------------------------------------
c = 'fixed-header-footer'
FH = '.hd{position:fixed;top:0;left:0;right:0;height:40px;line-height:40px;background:#1e3a8a;color:#fff;padding:0 10px}.ft{position:fixed;bottom:0;left:0;right:0;height:30px;line-height:30px;background:#475569;color:#fff;padding:0 10px}'
write(c, '01-fixed-header', FH + 'body{padding-top:50px}', '<div class="hd">Running header</div>' + bars(150))
write(c, '02-fixed-footer', FH + 'body{padding-bottom:40px}', '<div class="ft">Running footer</div>' + bars(150))
write(c, '03-fixed-header-and-footer', FH + 'body{padding:50px 0 40px}', '<div class="hd">Header</div><div class="ft">Footer</div>' + bars(150))
write(c, '04-page-margin-boxes', '@page{margin:0.9in 0.7in;@top-center{content:"Report title";font-size:11px;color:#1e3a8a}@bottom-center{content:"Page " counter(page) " of " counter(pages);font-size:11px}}', bars(150))
write(c, '05-margin-box-left-right', '@page{margin:0.9in;@top-left{content:"Left";font-size:11px}@top-right{content:"Right";font-size:11px}@bottom-left{content:"Confidential";font-size:11px}@bottom-right{content:counter(page);font-size:11px}}', bars(150))
write(c, '06-absolute-in-relative-body', 'body{position:relative}.ab{position:absolute;top:20px;right:20px;width:140px;height:60px;background:#fde047;border:2px solid #a16207}', '<div class="ab">stamp</div>' + bars(150))
write(c, '07-fixed-sidebar', '.sb{position:fixed;top:0;left:0;bottom:0;width:90px;background:#e2e8f0;border-right:2px solid #64748b}body{margin-left:100px}', '<div class="sb">menu</div>' + bars(150))
write(c, '08-fixed-watermark', '.wm{position:fixed;top:300px;left:100px;font-size:90px;color:rgba(220,38,38,.2);transform:rotate(-30deg)}', '<div class="wm">DRAFT</div>' + bars(150))
write(c, '09-header-with-tall-content', FH + '.hd{height:70px;line-height:70px}body{padding-top:80px}', '<div class="hd">Tall header</div>' + bars(150))
write(c, '10-footer-overlap-check', FH + 'body{padding-bottom:36px}.last{height:200px;background:#fecaca}', '<div class="ft">Footer</div>' + bars(60) + '<div class="last">last block</div>')
write(c, '11-sticky-header-in-table', 'th{position:sticky;top:0;background:#1e40af;color:#fff;height:30px}td{height:28px;border-bottom:1px solid #ddd}table{width:100%;border-collapse:collapse}', '<table><thead><tr><th>sticky head</th></tr></thead><tbody>' + ''.join(f'<tr><td>row {i}</td></tr>' for i in range(1, 70)) + '</tbody></table>')
write(c, '12-header-footer-and-forced-break', FH + 'body{padding:50px 0 40px}', '<div class="hd">Header</div><div class="ft">Footer</div>' + bars(10) + bars(10, label="after").replace('class="bar', 'style="break-before:page" class="bar', 1))

# ---- 8. complex tables: spans, colours, borders, outlines, shapes, lots of data ------------------------------------------
c = 'complex-tables'
TB = 'table{border-collapse:collapse;width:100%;font-size:12px}th,td{border:1px solid #64748b;padding:4px 8px;height:24px}th{background:#1e3a8a;color:#fff}.r{text-align:right}.c{text-align:center}.tot{background:#fef3c7;font-weight:bold}'
money = lambda i, j: f'{(i * 37 + j * 91) % 9000 + 100:,}.{(i * 7 + j) % 100:02d}'

# 1. two-level header: colspan groups over columns, rowspan on the first columns
head2 = '<thead><tr><th rowspan="2">Region</th><th rowspan="2">Product</th><th colspan="4">2025</th><th colspan="4">2026</th><th rowspan="2">Change</th></tr><tr>' + ''.join(f'<th>Q{q}</th>' for q in (1, 2, 3, 4)) * 2 + '</tr></thead>'
def body2(n):
    out = ''
    for r in range(n):
        out += f'<tr><td rowspan="3" style="background:#dbeafe;font-weight:bold">Region {r + 1}</td>'
        for k in range(3):
            if k:
                out += '<tr>'
            out += f'<td>Product {chr(65 + k)}</td>' + ''.join(f'<td class="r">{money(r * 3 + k, j)}</td>' for j in range(8)) + f'<td class="r" style="color:{"#166534" if (r + k) % 2 else "#991b1b"}">{(r * 5 + k * 3) % 17 - 8}%</td></tr>'
    return out
write(c, '01-two-level-header-region-rowspan', TB, f'<table>{head2}<tbody>{body2(14)}</tbody></table>')

# 2. financial statement: section rows span all columns, subtotal rows span the label, grand total
rows = ''
for sec in range(6):
    rows += f'<tr><td colspan="6" style="background:#e2e8f0;font-weight:bold">Section {sec + 1}</td></tr>'
    for i in range(7):
        rows += f'<tr><td>{sec + 1}.{i + 1}</td><td colspan="2">Line item {sec * 7 + i + 1}</td>' + ''.join(f'<td class="r">{money(sec * 7 + i, j)}</td>' for j in range(3)) + '</tr>'
    rows += f'<tr class="tot"><td colspan="3" class="r">Subtotal {sec + 1}</td>' + ''.join(f'<td class="r">{money(sec, j + 40)}</td>' for j in range(3)) + '</tr>'
rows += '<tr class="tot" style="background:#bbf7d0"><td colspan="3" class="r">Grand total</td><td class="r" colspan="3">1,234,567.89</td></tr>'
write(c, '02-financial-statement-section-rows', TB, '<table><thead><tr><th>No.</th><th colspan="2">Description</th><th>Budget</th><th>Actual</th><th>Variance</th></tr></thead><tbody>' + rows + '</tbody></table>')

# 3. timetable: coloured blocks that span several rows and columns
days = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri']
tt = '<table><thead><tr><th>Time</th>' + ''.join(f'<th>{d}</th>' for d in days) + '</tr></thead><tbody>'
skip = set()
blocks = {(0, 0): (2, 1, '#bfdbfe'), (1, 1): (3, 1, '#bbf7d0'), (3, 2): (2, 2, '#fecaca'), (0, 3): (4, 1, '#e9d5ff'), (6, 0): (2, 3, '#fde68a'), (8, 4): (3, 1, '#a5f3fc'), (5, 4): (2, 1, '#fbcfe8')}
for r in range(14):
    tt += f'<tr><td style="background:#f1f5f9;font-weight:bold">{8 + r}:00</td>'
    for cidx in range(5):
        if (r, cidx) in skip:
            continue
        if (r, cidx) in blocks:
            rs, cs, col = blocks[(r, cidx)]
            for dr in range(rs):
                for dc in range(cs):
                    if dr or dc:
                        skip.add((r + dr, cidx + dc))
            tt += f'<td rowspan="{rs}" colspan="{cs}" style="background:{col};text-align:center;vertical-align:middle">Session {r}{cidx}</td>'
        else:
            tt += '<td></td>'
    tt += '</tr>'
write(c, '03-timetable-coloured-blocks', TB + 'td{height:36px}', tt + '</tbody></table>')

# 4. invoice
inv = '<table><thead><tr><th>#</th><th style="width:40%">Item</th><th>Qty</th><th>Unit</th><th>Amount</th></tr></thead><tbody>' + ''.join(f'<tr><td>{i + 1}</td><td>Service {i + 1}</td><td class="r">{i % 5 + 1}</td><td class="r">{money(i, 3)}</td><td class="r">{money(i, 4)}</td></tr>' for i in range(24)) + '<tr><td colspan="3" rowspan="3" style="vertical-align:top">Thank you for your business.<br>Payment due in 30 days.</td><td class="r">Subtotal</td><td class="r">20,000.00</td></tr><tr><td class="r">Tax 12%</td><td class="r">2,400.00</td></tr><tr class="tot"><td class="r">Total</td><td class="r">22,400.00</td></tr></tbody></table>'
write(c, '04-invoice-with-spanning-totals', TB, inv)

# 5. heatmap: many small coloured cells
def heat(v):
    r = int(255 * v); b = int(255 * (1 - v)); return f'rgb({r},{int(120 * (1 - abs(2 * v - 1)))},{b})'
hm = '<table><thead><tr><th></th>' + ''.join(f'<th>C{j + 1}</th>' for j in range(14)) + '</tr></thead><tbody>' + ''.join(f'<tr><th style="background:#475569">R{i + 1}</th>' + ''.join(f'<td class="c" style="background:{heat(((i * 13 + j * 7) % 20) / 19)};color:#fff">{(i * 13 + j * 7) % 100}</td>' for j in range(14)) + '</tr>' for i in range(34)) + '</tbody></table>'
write(c, '05-heatmap-coloured-cells', TB + 'td,th{height:22px;padding:1px 4px}', hm)

# 6. pivot with three header levels
lvl = '<thead><tr><th rowspan="3">Group</th><th colspan="6">Year 1</th><th colspan="6">Year 2</th></tr><tr>' + ''.join(f'<th colspan="3">H{h}</th>' for h in range(1, 5)) + '</tr><tr>' + ''.join(f'<th>{m}</th>' for m in ('a', 'b', 'c')) * 4 + '</tr></thead>'
pv = f'<table>{lvl}<tbody>' + ''.join(f'<tr><td style="background:#e0f2fe">Group {i + 1}</td>' + ''.join(f'<td class="r">{(i * 11 + j * 3) % 90 + 10}</td>' for j in range(12)) + '</tr>' for i in range(46)) + '</tbody></table>'
write(c, '06-pivot-three-header-levels', TB, pv)

# 7. cell borders in different styles and colours
styles = ['solid', 'dashed', 'dotted', 'double', 'groove', 'ridge']
cols = ['#dc2626', '#2563eb', '#16a34a', '#9333ea', '#d97706', '#0891b2']
bt = '<table style="border-collapse:collapse"><tbody>' + ''.join('<tr>' + ''.join(f'<td style="border:{2 + (i + j) % 3}px {styles[(i + j) % 6]} {cols[(i * 2 + j) % 6]};height:34px;text-align:center">{styles[(i + j) % 6]}</td>' for j in range(6)) + '</tr>' for i in range(18)) + '</tbody></table>'
write(c, '07-mixed-border-styles-and-colours', 'table{width:100%;font-size:12px}', bt)

# 8. separate borders, rounded cells, spacing
rt = '<table style="border-collapse:separate;border-spacing:6px"><thead><tr>' + ''.join(f'<th style="border-radius:8px;background:{cols[j]};color:#fff;height:30px">H{j + 1}</th>' for j in range(6)) + '</tr></thead><tbody>' + ''.join('<tr>' + ''.join(f'<td style="border-radius:{4 + (i + j) % 4 * 6}px;background:{["#fee2e2", "#dbeafe", "#dcfce7", "#f3e8ff", "#fef3c7", "#cffafe"][(i + j) % 6]};border:1px solid #94a3b8;height:28px;text-align:center">{i + 1}.{j + 1}</td>' for j in range(6)) + '</tr>' for i in range(22)) + '</tbody></table>'
write(c, '08-separate-borders-rounded-cells', 'table{width:100%;font-size:12px}', rt)

# 9. outlines on the table and some cells
ot = '<table style="outline:3px solid #1e3a8a;outline-offset:4px;margin:10px 0"><thead><tr><th>A</th><th>B</th><th>C</th><th>D</th></tr></thead><tbody>' + ''.join(f'<tr><td>{i}</td><td style="outline:2px solid #dc2626;outline-offset:-3px">{i * 2}</td><td>{i * 3}</td><td style="outline:2px dashed #16a34a;outline-offset:-3px">{i * 4}</td></tr>' for i in range(1, 30)) + '</tbody></table>'
write(c, '09-table-and-cell-outlines', TB, ot)

# 10. tables nested in cells
nest = '<table><thead><tr><th>Order</th><th>Items</th><th>Status</th></tr></thead><tbody>' + ''.join(f'<tr><td>#{1000 + i}</td><td><table style="width:100%"><tr><td style="background:#f1f5f9">Item A</td><td class="r">{i + 2}</td></tr><tr><td style="background:#f1f5f9">Item B</td><td class="r">{i + 5}</td></tr></table></td><td style="background:{["#bbf7d0", "#fde68a", "#fecaca"][i % 3]}">{["Shipped", "Pending", "Cancelled"][i % 3]}</td></tr>' for i in range(16)) + '</tbody></table>'
write(c, '10-nested-tables-in-cells', TB, nest)

# 11. vertical alignment inside rowspans, gradient backgrounds, colour text
va = '<table><thead><tr><th>Group</th><th>Top</th><th>Middle</th><th>Bottom</th><th>Baseline</th></tr></thead><tbody>' + ''.join(f'<tr><td rowspan="2" style="background:linear-gradient(#bfdbfe,#1e3a8a);color:#fff;height:80px;vertical-align:middle">G{i + 1}</td><td style="vertical-align:top;color:#b91c1c">top</td><td style="vertical-align:middle;color:#15803d">middle</td><td style="vertical-align:bottom;color:#1d4ed8">bottom</td><td style="vertical-align:baseline">baseline</td></tr><tr><td colspan="2" style="background:#fef9c3;height:50px;vertical-align:bottom;text-align:right">span 2 columns, bottom right</td><td colspan="2" style="background:#fae8ff;vertical-align:top">span 2, top</td></tr>' for i in range(9)) + '</tbody></table>'
write(c, '11-vertical-align-gradients-spans', TB, va)

# 12. long table over several pages, two header rows repeating, group cells with rowspan crossing the page breaks
lg = '<table><thead><tr><th rowspan="2">Dept</th><th rowspan="2">Employee</th><th colspan="3">Score</th></tr><tr><th>A</th><th>B</th><th>C</th></tr></thead><tbody>'
for d in range(10):
    for e in range(14):
        lg += '<tr>' + (f'<td rowspan="14" style="background:{cols[d % 6]}33;font-weight:bold;vertical-align:top">Dept {d + 1}</td>' if e == 0 else '') + f'<td>Employee {d * 14 + e + 1}</td>' + ''.join(f'<td class="r">{(d * 14 + e) * (j + 3) % 100}</td>' for j in range(3)) + '</tr>'
lg += '</tbody></table>'
write(c, '12-long-table-rowspan-across-pages', TB, lg)

# 13. percentage and fixed widths with colspans, empty cells
fx = '<table style="table-layout:fixed"><colgroup><col style="width:10%"><col style="width:30%"><col style="width:20%"><col style="width:20%"><col style="width:20%"></colgroup><thead><tr><th>#</th><th colspan="2">Name and address</th><th colspan="2">Contact</th></tr></thead><tbody>' + ''.join(f'<tr><td>{i}</td><td>Person {i}</td><td>{"" if i % 3 == 0 else f"Street {i}"}</td><td colspan="{1 + i % 2}">{"" if i % 4 == 0 else f"555-{i:04d}"}</td>' + ('' if i % 2 else '<td></td>') + '</tr>' for i in range(1, 50)) + '</tbody></table>'
write(c, '13-fixed-layout-colspans-and-empty-cells', TB, fx)

# 14. caption, colgroup column colours, tfoot
cg = '<table><caption style="caption-side:top;font-weight:bold;padding:6px;text-align:left">Sales by month</caption><colgroup><col style="background:#f1f5f9"><col span="3" style="background:#ecfdf5"><col span="2" style="background:#fff7ed"></colgroup><thead><tr><th>Month</th><th colspan="3">Units</th><th colspan="2">Revenue</th></tr></thead><tbody>' + ''.join(f'<tr><td>M{i + 1}</td>' + ''.join(f'<td class="r">{(i * 17 + j * 5) % 500}</td>' for j in range(5)) + '</tr>' for i in range(36)) + '</tbody><tfoot><tr class="tot"><td>Total</td><td colspan="3" class="r">sum</td><td colspan="2" class="r">sum</td></tr></tfoot></table>'
write(c, '14-caption-colgroup-tfoot', TB, cg)

# 15. very wide table, small font
wd = '<table style="font-size:9px"><thead><tr><th>ID</th>' + ''.join(f'<th>F{j + 1}</th>' for j in range(18)) + '</tr></thead><tbody>' + ''.join(f'<tr><td>{i + 1}</td>' + ''.join(f'<td class="r">{(i * 7 + j * 13) % 1000}</td>' for j in range(18)) + '</tr>' for i in range(60)) + '</tbody></table>'
write(c, '15-wide-table-18-columns', TB + 'td,th{padding:2px 3px;height:18px}', wd)

# 16. a checker of coloured merged cells (spans in both directions on every row)
ck = '<table><tbody>' + ''.join('<tr>' + ''.join(f'<td colspan="{1 + (i + j) % 2}" rowspan="1" style="background:{cols[(i + j) % 6]}22;height:30px;text-align:center">{i}-{j}</td>' for j in range(0, 8, 1) if (i + j) % 3 != 2 or j % 2 == 0) + '</tr>' for i in range(30)) + '</tbody></table>'
write(c, '16-irregular-merged-cells', TB + 'table{table-layout:auto}', ck)

print('wrote', sum(len(os.listdir(os.path.join(OUT, c))) for c in CATEGORIES), 'cases')
