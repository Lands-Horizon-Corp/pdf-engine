// pdf.fromElement(element): what is on screen, as the options for generate(). Browser pages only.
//
// The engine makes a PDF from HTML it is given, it does not look at the page the user has open. This copies an element
// the way it looks: every element gets its computed styles written inline (so the page's stylesheets, classes and
// custom properties are not needed), canvases and images become data, inline SVG is kept with its computed paint,
// form controls become text, simple ::before and ::after text becomes real text, and the @font-face rules of the fonts
// in use are carried along. The result is one self-contained document and the page size to match.
//
// It is a snapshot, not a browser: scripts do not run in the PDF, `position: fixed` and `sticky` are dropped, and CSS
// the engine does not draw (outline, filters, floats) is left as it is.

const SKIP = new Set(['SCRIPT', 'STYLE', 'NOSCRIPT', 'LINK', 'META', 'TEMPLATE', 'IFRAME', 'OBJECT', 'EMBED', 'VIDEO', 'AUDIO', 'SOURCE', 'TRACK', 'DIALOG', 'HEAD', 'TITLE']);
const MAX_NODES = 30_000;

// Properties copied from the computed style. A value equal to the browser default for that tag is not written.
const PROPS = [
  'display', 'visibility', 'opacity', 'overflow-x', 'overflow-y', 'box-sizing',
  'flex-direction', 'flex-wrap', 'flex-grow', 'flex-shrink', 'flex-basis', 'justify-content', 'align-items', 'align-content', 'align-self', 'order', 'row-gap', 'column-gap',
  'grid-template-columns', 'grid-template-rows', 'grid-auto-flow', 'grid-auto-columns', 'grid-auto-rows', 'grid-column-start', 'grid-column-end', 'grid-row-start', 'grid-row-end',
  'margin-top', 'margin-right', 'margin-bottom', 'margin-left', 'padding-top', 'padding-right', 'padding-bottom', 'padding-left',
  'border-top-width', 'border-right-width', 'border-bottom-width', 'border-left-width', 'border-top-style', 'border-right-style', 'border-bottom-style', 'border-left-style',
  'border-top-color', 'border-right-color', 'border-bottom-color', 'border-left-color',
  'border-top-left-radius', 'border-top-right-radius', 'border-bottom-right-radius', 'border-bottom-left-radius',
  'color', 'background-color', 'box-shadow', 'transform', 'transform-origin',
  'font-family', 'font-size', 'font-weight', 'font-style', 'line-height', 'letter-spacing', 'word-spacing', 'text-align', 'text-indent', 'text-transform',
  'text-decoration-line', 'text-decoration-color', 'text-decoration-style', 'white-space', 'word-break', 'overflow-wrap', 'vertical-align',
  'list-style-type', 'list-style-position', 'border-collapse', 'border-spacing', 'table-layout', 'caption-side', 'empty-cells',
];
const POSITIONED = ['top', 'right', 'bottom', 'left', 'width', 'height'];
const SVG_PROPS = ['fill', 'fill-opacity', 'fill-rule', 'stroke', 'stroke-width', 'stroke-opacity', 'stroke-dasharray', 'stroke-linecap', 'stroke-linejoin', 'opacity', 'font-family', 'font-size', 'font-weight', 'text-anchor', 'visibility', 'display', 'color'];

let defaults = null;

/** Browser defaults per tag, read once from a blank document, so only what the page changed is written out. */
function defaultsFor(tag, namespace) {
  if (!defaults) {
    const frame = document.createElement('iframe');
    frame.setAttribute('aria-hidden', 'true');
    frame.style.cssText = 'position:absolute;left:-9999px;top:0;width:0;height:0;border:0;visibility:hidden';
    document.body.append(frame);
    defaults = { frame, doc: frame.contentDocument, map: new Map() };
  }
  const key = `${namespace}:${tag}`;
  let found = defaults.map.get(key);
  if (!found) {
    const doc = defaults.doc;
    const el = namespace === 'svg' ? doc.createElementNS('http://www.w3.org/2000/svg', tag) : doc.createElement(tag);
    doc.body.append(el);
    const cs = doc.defaultView.getComputedStyle(el);
    found = Object.fromEntries([...PROPS, ...SVG_PROPS, ...POSITIONED].map((p) => [p, cs.getPropertyValue(p)]));
    el.remove();
    defaults.map.set(key, found);
  }
  return found;
}

function releaseDefaults() {
  defaults?.frame.remove();
  defaults = null;
}

function cssFor(el, cs, tagDefaults, isRoot) {
  const out = [];
  for (const p of PROPS) {
    const v = cs.getPropertyValue(p);
    if (v && v !== tagDefaults[p]) out.push(`${p}:${v}`);
  }
  const pos = cs.getPropertyValue('position');
  if (pos === 'absolute' || pos === 'relative') out.push(`position:${pos}`);
  if (pos === 'absolute') for (const p of POSITIONED) out.push(`${p}:${cs.getPropertyValue(p)}`);
  // Only gradients can be painted as a background (a url() image cannot).
  const bg = cs.getPropertyValue('background-image');
  if (/^(repeating-)?(linear|radial)-gradient/.test(bg)) out.push(`background-image:${bg}`);
  if (isRoot) out.push('margin:0');
  return out.join(';');
}

function svgCss(cs, tagDefaults) {
  const out = [];
  for (const p of SVG_PROPS) {
    const v = cs.getPropertyValue(p);
    if (v && v !== tagDefaults[p]) out.push(`${p}:${v}`);
  }
  return out.join(';');
}

function quotedContent(content) {
  const m = /^"((?:[^"\\]|\\.)*)"$/.exec(content);
  return m ? m[1].replace(/\\(.)/g, '$1') : null;
}

function pseudo(el, which, win) {
  const cs = win.getComputedStyle(el, which);
  const text = quotedContent(cs.getPropertyValue('content'));
  if (!text || cs.getPropertyValue('display') === 'none') return null;
  const span = document.createElement('span');
  span.textContent = text;
  const d = defaultsFor('span', 'html');
  const css = [];
  for (const p of PROPS) {
    const v = cs.getPropertyValue(p);
    if (v && v !== d[p]) css.push(`${p}:${v}`);
  }
  span.setAttribute('style', css.join(';'));
  return span;
}

async function imageData(img) {
  try {
    await img.decode();
    const c = document.createElement('canvas');
    c.width = img.naturalWidth;
    c.height = img.naturalHeight;
    c.getContext('2d').drawImage(img, 0, 0);
    return c.toDataURL('image/png');
  } catch {
    return null; // cross-origin without CORS, or not decodable: keep the address
  }
}

function checkbox(checked, radio) {
  const box = document.createElement('span');
  const dot = checked ? (radio ? 'radial-gradient(#333 45%, #fff 50%)' : 'linear-gradient(#333,#333)') : 'none';
  box.setAttribute('style', `display:inline-block;width:0.8em;height:0.8em;border:1px solid #333;border-radius:${radio ? '50%' : '2px'};vertical-align:middle;${dot === 'none' ? '' : `background-image:${dot}`}`);
  return box;
}

/** Copy `source` into a styled clone. Returns null for something that is not part of the picture. */
async function copy(source, ctx, isRoot = false) {
  if (++ctx.count > MAX_NODES) throw new Error(`the element has more than ${MAX_NODES} nodes`);
  if (source.nodeType === Node.TEXT_NODE) return document.createTextNode(source.nodeValue);
  if (source.nodeType !== Node.ELEMENT_NODE) return null;
  const tag = source.tagName.toUpperCase();
  if (SKIP.has(tag)) return null;
  const win = source.ownerDocument.defaultView;
  const cs = win.getComputedStyle(source);
  if (cs.getPropertyValue('display') === 'none') return null;
  const position = cs.getPropertyValue('position');
  if (position === 'fixed' || position === 'sticky') return null;
  ctx.families.add(cs.getPropertyValue('font-family'));

  const isSvg = source.namespaceURI === 'http://www.w3.org/2000/svg';
  if (isSvg && tag !== 'SVG') {
    // A child of an inline SVG: keep it as it is, with the paint the page's CSS gave it.
    const el = source.cloneNode(false);
    const css = svgCss(cs, defaultsFor(source.localName, 'svg'));
    if (css) el.setAttribute('style', `${source.getAttribute('style') ?? ''};${css}`);
    for (const child of source.childNodes) {
      const c = await copy(child, ctx);
      if (c) el.append(c);
    }
    return el;
  }
  if (isSvg) {
    const el = source.cloneNode(false);
    const rect = source.getBoundingClientRect();
    if (!el.getAttribute('width')) el.setAttribute('width', String(Math.round(rect.width)));
    if (!el.getAttribute('height')) el.setAttribute('height', String(Math.round(rect.height)));
    // An SVG a pixel wider than the space it sits in is not drawn, so let it shrink.
    el.setAttribute('style', `display:${cs.getPropertyValue('display') === 'inline' ? 'inline-block' : cs.getPropertyValue('display')};max-width:100%`);
    for (const child of source.childNodes) {
      const c = await copy(child, ctx);
      if (c) el.append(c);
    }
    return el;
  }

  if (tag === 'CANVAS') {
    const img = document.createElement('img');
    let url = null;
    try {
      url = source.toDataURL('image/png');
    } catch {
      return null; // a canvas holding cross-origin pixels cannot be read
    }
    const rect = source.getBoundingClientRect();
    img.setAttribute('src', url);
    img.setAttribute('width', String(Math.round(rect.width)));
    img.setAttribute('height', String(Math.round(rect.height)));
    return img;
  }
  if (tag === 'IMG') {
    const img = document.createElement('img');
    const rect = source.getBoundingClientRect();
    const alt = source.getAttribute('alt');
    if (alt) img.setAttribute('alt', alt);
    const data = ctx.options.images === 'link' ? null : await imageData(source);
    img.setAttribute('src', data ?? source.currentSrc ?? source.src);
    img.setAttribute('width', String(Math.round(rect.width)));
    img.setAttribute('height', String(Math.round(rect.height)));
    const css = cssFor(source, cs, defaultsFor('img', 'html'), false);
    if (css) img.setAttribute('style', css);
    return img;
  }
  if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT' || tag === 'BUTTON') {
    const type = (source.getAttribute('type') || '').toLowerCase();
    if (tag === 'INPUT' && (type === 'checkbox' || type === 'radio')) return checkbox(source.checked, type === 'radio');
    if (tag === 'INPUT' && type === 'hidden') return null;
    const span = document.createElement('span');
    span.setAttribute('style', `${cssFor(source, cs, defaultsFor('span', 'html'), false)};display:inline-block`);
    if (tag === 'SELECT') span.textContent = [...source.selectedOptions].map((o) => o.textContent).join(', ');
    else if (tag === 'BUTTON') span.textContent = source.textContent;
    else span.textContent = source.value || source.getAttribute('placeholder') || '';
    return span;
  }

  const el = document.createElement(source.localName);
  // A table made a block (the usual "scroll sideways" trick) is still a table of rows: a block cannot lay out table parts.
  const forceTable = tag === 'TABLE' && !/^(table|inline-table)$/.test(cs.getPropertyValue('display'));
  // A closed <details> shows its <summary> only; an open one keeps its content.
  const closedDetails = tag === 'DETAILS' && !source.open;
  if (tag === 'DETAILS' && source.open) el.setAttribute('open', '');
  const keep = ['href', 'colspan', 'rowspan', 'title', 'lang', 'start', 'value'];
  if (tag === 'OL' || tag === 'UL' || tag === 'LI') keep.push('type');
  for (const name of keep) {
    if (source.hasAttribute(name)) el.setAttribute(name, name === 'href' ? source.href : source.getAttribute(name));
  }
  let css = cssFor(source, cs, defaultsFor(source.localName, 'html'), isRoot);
  if (forceTable) css = `${css.replace(/(^|;)display:[^;]*/, '$1display:table')};display:table`;
  if (css) el.setAttribute('style', css);

  const before = pseudo(source, '::before', win);
  if (before) el.append(before);
  for (const child of source.childNodes) {
    if (closedDetails && !(child.nodeType === Node.ELEMENT_NODE && child.tagName === 'SUMMARY')) continue;
    const c = await copy(child, ctx);
    if (c) el.append(c);
  }
  const after = pseudo(source, '::after', win);
  if (after) el.append(after);
  return el;
}

const unquote = (s) => s.trim().replace(/^["']|["']$/g, '');

/** The @font-face rules of the families the snapshot uses, with absolute font addresses. */
function fontFaces(families) {
  const used = new Set();
  for (const list of families) for (const f of list.split(',')) used.add(unquote(f).toLowerCase());
  const out = [];
  const visit = (rules, base) => {
    for (const rule of rules) {
      if (rule.type === CSSRule.FONT_FACE_RULE) {
        const family = unquote(rule.style.getPropertyValue('font-family'));
        if (!used.has(family.toLowerCase())) continue;
        const src = rule.style.getPropertyValue('src').replace(/url\(\s*(["']?)([^"')]+)\1\s*\)/g, (m, q, u) => (u.startsWith('data:') ? m : `url("${new URL(u, base).href}")`));
        const weight = rule.style.getPropertyValue('font-weight');
        const style = rule.style.getPropertyValue('font-style');
        out.push(`@font-face{font-family:"${family}";src:${src};${weight ? `font-weight:${weight};` : ''}${style ? `font-style:${style};` : ''}}`);
      } else if (rule.cssRules && (rule.type === CSSRule.MEDIA_RULE || rule.type === CSSRule.SUPPORTS_RULE)) {
        visit(rule.cssRules, base);
      }
    }
  };
  for (const sheet of document.styleSheets) {
    let rules;
    try {
      rules = sheet.cssRules;
    } catch {
      continue; // a stylesheet from another origin cannot be read
    }
    visit(rules, sheet.href || document.baseURI);
  }
  return out.join('\n');
}

/**
 * The HTML of a snapshot of `element`, and the size of the page that fits it.
 * @returns {Promise<{ html: string, width: number, height: number }>}
 */
export async function snapshot(element, options = {}) {
  const el = typeof element === 'string' ? document.querySelector(element) : element;
  if (!(el instanceof Element)) throw new Error(`fromElement(): nothing matches ${JSON.stringify(element)}`);
  const margin = Number(options.margin ?? 16);
  const ctx = { count: 0, families: new Set(), options };
  const rect = el.getBoundingClientRect();
  try {
    const clone = await copy(el, ctx, true);
    if (!clone) throw new Error('fromElement(): the element is not visible');
    const width = Math.max(100, Math.ceil(rect.width));
    const faces = options.fonts === false ? '' : fontFaces(ctx.families);
    const html = `${faces ? `<style>${faces}</style>` : ''}<div style="box-sizing:border-box;width:${width}px;padding:${margin}px;background:#fff">${clone.outerHTML}</div>`;
    const pageWidth = width + 2 * margin;
    return { html, width: pageWidth, height: Math.round((pageWidth * 11) / 8.5) };
  } finally {
    releaseDefaults();
  }
}

/** Template text that the template language will not read: `{{`, `{%` and `{#` in the page stay as they are. */
export function asLiteralTemplate(html) {
  return `{% raw %}${html.replace(/\{%(\s*-?\s*)endraw/gi, '{%$1end​raw')}{% endraw %}`;
}

/** Options for `generate()` (or `download()`, `open()`, `preview()`) that make a PDF of `element` as it looks. */
export async function elementToOptions(element, options = {}) {
  const { margin, images, fonts, ...rest } = options;
  const { html, width, height } = await snapshot(element, { margin, images, fonts });
  return { width: `${width}px`, height: `${height}px`, ...rest, template: asLiteralTemplate(html) };
}
