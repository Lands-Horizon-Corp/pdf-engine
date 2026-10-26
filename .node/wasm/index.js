/**
 * HTML to PDF in the browser. The pdf-engine renderer, compiled to WebAssembly: no server, no
 * network round trip, nothing leaves the page.
 *
 *   import { PDFEngine } from '@lands-horizon-corp/pdf-engine-wasm';
 *   const pdf = new PDFEngine();
 *   const bytes = await pdf.generate({ template: '<h1>Invoice {{ no }}</h1>', data: { no: 42 } });
 *
 * `generate()` runs on the calling thread (a few milliseconds for a receipt). Call it from a Web
 * Worker if you render large documents.
 */
import { WorkerPool } from './pool.js';
import { ServerPool, choose, isRetryable, pruneData, requestBytes, serversOf } from './routing.js';
import init, { Engine, set_compression, version as engineVersion } from './dist/pdf_engine_wasm.js';

const isNode = typeof process !== 'undefined' && !!process.versions?.node && typeof window === 'undefined';

/** An error from the engine (it carries the engine's `code`, `status`, `hint`, `line`) or from this client. */
export class PDFEngineError extends Error {
  constructor({ code, message, status, hint, line }) {
    super(message);
    this.name = 'PDFEngineError';
    this.code = code;
    if (status !== undefined) this.status = status;
    if (hint !== undefined) this.hint = hint;
    if (line !== undefined) this.line = line;
  }
}

const clientError = (code, message, hint) => new PDFEngineError({ code, message, hint });

/** The Latin fonts (Latin, Cyrillic, Greek, Vietnamese) that ship with the package. */
const defaultFontUrls = () => [
  new URL('./fonts/NotoSans-Regular.ttf', import.meta.url),
  new URL('./fonts/NotoSans-Bold.ttf', import.meta.url),
];

/** Import a Node built-in without bundlers trying to resolve it (this only runs under Node). */
const nodeImport = (name) => import(/* webpackIgnore: true */ /* @vite-ignore */ name);

const bytesCache = new Map();

/** Font or wasm bytes from a URL, a file path (Node), a Blob, or bytes already in memory. */
async function loadBytes(source, fetchImpl, cache = true) {
  if (source instanceof Uint8Array) return source;
  if (source instanceof ArrayBuffer) return new Uint8Array(source);
  if (ArrayBuffer.isView(source)) return new Uint8Array(source.buffer, source.byteOffset, source.byteLength);
  if (typeof Blob !== 'undefined' && source instanceof Blob) return new Uint8Array(await source.arrayBuffer());

  const key = String(source);
  if (!cache) return fetchBytes(source, key, fetchImpl);
  if (!bytesCache.has(key)) {
    bytesCache.set(key, fetchBytes(source, key, fetchImpl));
    bytesCache.get(key).catch(() => bytesCache.delete(key));
  }
  return bytesCache.get(key);
}

async function fetchBytes(source, key, fetchImpl) {
  // A Windows path (C:\\dir\\file) starts like a URL scheme, so it is recognised first.
  if (isNode && (source instanceof URL ? source.protocol === 'file:' : /^[A-Za-z]:[\\/]/.test(key) || !/^[a-z][a-z0-9+.-]*:/i.test(key) || key.startsWith('file:'))) {
    const { readFile } = await nodeImport('node:fs/promises');
    const { fileURLToPath } = await nodeImport('node:url');
    return new Uint8Array(await readFile(source instanceof URL || key.startsWith('file:') ? fileURLToPath(source) : key));
  }
  const response = await (fetchImpl || fetch)(source);
  if (!response.ok) throw new Error(`${response.status} ${response.statusText} for ${key}`);
  return new Uint8Array(await response.arrayBuffer());
}

const MAX_IMAGES = 200;
/** A template that may need something fetched before it renders: an `<img>` or an `@font-face` rule. */
const NEEDS_FETCH = /<img\b|@font-face/i;
const MAX_IMAGE_BYTES = 5 * 1024 * 1024;
const MAX_FONT_FACES = 16;
const MAX_FONT_BYTES = 16 * 1024 * 1024;

const dataUriBytes = (uri) => {
  const comma = uri.indexOf(',');
  if (comma < 0) return null;
  if (!/;base64$/i.test(uri.slice(0, comma))) return new TextEncoder().encode(decodeURIComponent(uri.slice(comma + 1)));
  const bin = atob(uri.slice(comma + 1));
  return Uint8Array.from(bin, (c) => c.charCodeAt(0));
};

/**
 * The bytes for one `<img src>`: a name in the `images` map (bytes, Blob, File, URL or data: URI),
 * else the `src` itself as a URL (http(s), blob:, a relative path; a file path under Node).
 * Resolves `null` for a `src` that is not an image source; throws a readable reason if it fails.
 */
async function loadImage(src, images, fetchImpl, allowLocalFiles) {
  const mapped = images && Object.hasOwn(images, src);
  const source = mapped ? images[src] : src;
  if (typeof source === 'string' && source.startsWith('data:')) return dataUriBytes(source);
  if (typeof source === 'string' && /^(javascript|about):/i.test(source)) return null;
  // Under Node a bare path or file: URL would read the disk; a template is not trusted with that.
  if (!mapped && !allowLocalFiles && isNode && typeof source === 'string' && !/^(https?|blob):/i.test(source)) {
    throw new Error('local files are not read unless `allowLocalFiles: true` (or pass the file in `images`)');
  }
  const bytes = await loadBytes(source, fetchImpl, false);
  if (bytes.length > MAX_IMAGE_BYTES) throw new Error(`larger than ${MAX_IMAGE_BYTES / 1024 / 1024} MB`);
  return bytes;
}

/** The bytes for one `@font-face` `src` (a URL; a file path only with `allowLocalFiles` under Node). */
async function loadFont(src, fetchImpl, allowLocalFiles) {
  if (/^(javascript|about):/i.test(src)) return null;
  if (!allowLocalFiles && isNode && !/^(https?|blob):/i.test(src)) {
    throw new Error('local files are not read unless `allowLocalFiles: true`');
  }
  const bytes = await loadBytes(src, fetchImpl, false);
  if (bytes.length > MAX_FONT_BYTES) throw new Error(`larger than ${MAX_FONT_BYTES / 1024 / 1024} MB`);
  return bytes;
}

function browserBlobUrl(bytes) {
  if (typeof document === 'undefined' || typeof URL === 'undefined' || !URL.createObjectURL) {
    throw clientError('NOT_IN_BROWSER', 'download(), open() and print() need a browser page. Use generate() and save the bytes yourself.');
  }
  return URL.createObjectURL(new Blob([bytes], { type: 'application/pdf' }));
}

let wasmReady = null;
let wasmKey = null;

/** Instantiate the WebAssembly module once per page. */
function ensureWasm(source, fetchImpl) {
  const key = source ? String(source instanceof Uint8Array ? 'bytes' : source) : 'default';
  if (!wasmReady || wasmKey !== key) {
    wasmKey = key;
    wasmReady = (async () => {
      try {
        if (source) return await init({ module_or_path: await loadBytes(source, fetchImpl) });
        // Node cannot fetch() a file: URL; the browser default (fetch next to the glue) is fine.
        if (isNode) {
          return await init({ module_or_path: await loadBytes(new URL('./dist/pdf_engine_wasm_bg.wasm', import.meta.url)) });
        }
        return await init();
      } catch (e) {
        wasmReady = null;
        throw clientError(
          'WASM_LOAD_FAILED',
          `Could not load the pdf-engine WebAssembly module: ${e?.message || e}`,
          'Serve the .wasm file with the application/wasm type, or pass { wasm: url } (see the README).',
        );
      }
    })();
  }
  return wasmReady;
}

const COMPRESSION_LEVELS = { fast: 0, balanced: 1, small: 6 };
let activeCompression = 'fast';

/** The fonts are process-wide inside WebAssembly; remember which instance's set is active. */
let activeFonts = null;

function toError(e) {
  if (typeof e === 'string') {
    try {
      const o = JSON.parse(e);
      if (o && typeof o === 'object' && o.code) return new PDFEngineError({ ...o, message: o.error });
    } catch {
      /* not JSON: fall through */
    }
  }
  return new PDFEngineError({ code: 'INTERNAL_ERROR', message: e?.message || String(e), status: 500 });
}

export class PDFEngine {
  #options;
  #engine = null;
  #starting = null;
  #fontBytes = [];
  #closed = false;

  /**
   * @param {{ fonts?: Array<string|URL|ArrayBuffer|Uint8Array|Blob>, defaultFonts?: boolean,
   *           wasm?: string|URL|ArrayBuffer|Uint8Array, fetch?: typeof fetch,
   *           url?: string, token?: string }} [options]
   */
  constructor(options = {}) {
    if (Array.isArray(options)) options = { fonts: options };
    this.#options = { defaultFonts: true, ...options, fonts: (options.fonts || []).filter((f) => f !== '' && f != null) };
  }

  #workerThreads = null;

  /** How many workers to render with: `workers: 3`, or `'auto'` (cores minus one, at most 4). 0 = this thread. */
  #workerCount() {
    const w = this.#options.workers;
    if (!w) return 0;
    if (w === 'auto') {
      const cores = (typeof navigator !== 'undefined' && navigator.hardwareConcurrency) || 2;
      return Math.max(1, Math.min(4, cores - 1));
    }
    if (!Number.isInteger(w) || w < 0 || w > 16) throw clientError('INVALID_FIELD', `workers must be a whole number from 0 to 16, or 'auto', not ${JSON.stringify(w)}.`);
    return w;
  }

  /** Relative URLs mean something different inside a worker; make them absolute against the page. */
  #absolute(value) {
    if (value instanceof URL) return value.href;
    if (typeof value === 'string' && typeof document !== 'undefined' && !/^[a-z][a-z0-9+.-]*:/i.test(value)) return new URL(value, document.baseURI).href;
    return value;
  }

  #workerPool(count) {
    if (!this.#workerThreads) {
      const { workers, fetch: _fetch, url, token, pick, autoMaxBytes, fallback, onImageError, ...rest } = this.#options;
      const init = { ...rest, fonts: this.#options.fonts.map((f) => this.#absolute(f)), wasm: rest.wasm === undefined ? undefined : this.#absolute(rest.wasm) };
      this.#workerThreads = new WorkerPool(count, init, (e) => new PDFEngineError({ ...e, message: e.message }));
    }
    return this.#workerThreads;
  }

  /**
   * Render HTML to a PDF. Resolves with the PDF bytes.
   *
   * Runs in this page (WebAssembly) unless a server `url` is given, here or in the constructor;
   * then the request goes to that pdf-engine server instead and no WebAssembly is loaded.
   */
  async generate(options) {
    if (!options || typeof options !== 'object') {
      throw clientError('MISSING_FIELD', 'generate() needs an options object with a `template`.');
    }
    const servers = serversOf({ url: options.url ?? this.#options.url, token: options.token ?? this.#options.token });
    // Only the keys the template uses are sent on (to the renderer or over the network).
    if (options.pruneData ?? this.#options.pruneData ?? true) options = { ...options, data: pruneData(options.template, options.data) };
    // With servers and no `pick`, documents go to the server (as before); `pick: 'auto'` also keeps working offline.
    const pick = options.pick ?? this.#options.pick ?? (servers.length ? 'server' : 'browser');
    const where = choose({
      pick,
      autoMaxBytes: options.autoMaxBytes ?? this.#options.autoMaxBytes,
      hasServers: servers.length > 0,
      online: typeof navigator === 'undefined' || navigator.onLine !== false,
      size: () => requestBytes(options),
    });
    if (where === 'browser') return this.#generateLocal(options);
    try {
      return await this.#pool(servers).run((server) => this.#generateRemote(server.url, server.token, options));
    } catch (e) {
      // Unreachable or overloaded servers: with `pick: 'auto'` (or `fallback: true`) the page renders it.
      if (!(options.fallback ?? this.#options.fallback ?? pick === 'auto') || !isRetryable(e)) throw e;
      return this.#generateLocal(options);
    }
  }

  #pools = new Map();

  #pool(servers) {
    const key = JSON.stringify(servers);
    if (!this.#pools.has(key)) this.#pools.set(key, new ServerPool(servers));
    return this.#pools.get(key);
  }

  async #generateLocal(options) {
    const workers = this.#workerCount();
    if (workers > 0) {
      if (this.#closed) throw clientError('ENGINE_CLOSED', 'This PDFEngine was closed; create a new one.');
      const { url, token, pick, autoMaxBytes, fallback, onImageError, fetch: _fetch, ...rest } = options;
      const images = rest.images && Object.fromEntries(Object.entries(rest.images).map(([k, v]) => [k, this.#absolute(v)]));
      const send = { ...rest, images, partials: this.#partials(options) };
      const warn = onImageError || this.#options.onImageError || ((src, reason) => console.warn(`pdf-engine: skipped image ${String(src).slice(0, 100)}: ${reason}`));
      return this.#workerPool(workers).generate(send, warn);
    }
    const { template, data, password, width, height, orientation } = options;
    await this.#start();
    this.#applyFonts();
    const compression = options.compression ?? this.#options.compression ?? 'fast';
    if (compression !== activeCompression) {
      if (!(compression in COMPRESSION_LEVELS)) throw clientError('INVALID_FIELD', `compression must be 'fast', 'balanced' or 'small', not ${JSON.stringify(compression)}.`);
      set_compression(COMPRESSION_LEVELS[compression]);
      activeCompression = compression;
    }
    const request = JSON.stringify({ template, data: data === undefined ? {} : data, password: password || undefined, width, height, orientation, partials: this.#partials(options) });
    try {
      // Images in data: URIs need nothing more. Any other <img src> (a URL, blob:, a name in
      // `images`) is fetched first, so a template without them keeps the one-call fast path.
      // The same for an `@font-face` rule: a font in a `data:` URI needs nothing more, a URL is fetched.
      if (typeof template === 'string' && (options.images || NEEDS_FETCH.test(template) || NEEDS_FETCH.test(Object.values({ ...this.#options.partials, ...options.partials }).join('\n')))) {
        return await this.#generateWithImages(request, options);
      }
      return this.#engine.generate(request);
    } catch (e) {
      if (typeof WebAssembly !== 'undefined' && e instanceof WebAssembly.RuntimeError) {
        // A trap (a bug in the engine) leaves the module unusable: start over on the next call.
        this.#engine = null;
        this.#starting = null;
        wasmReady = null;
        activeFonts = null;
        activeCompression = 'fast';
        throw new PDFEngineError({ code: 'RENDER_FAILED', message: 'The renderer crashed; it will restart on the next call.', status: 500 });
      }
      throw toError(e);
    }
  }

  async #generateWithImages(request, options) {
    const { id, sources, fontSources } = JSON.parse(this.#engine.prepare(request));
    if (sources.length === 0 && fontSources.length === 0) return this.#engine.render_prepared(id, [], [], [], []);
    const fetchImpl = options.fetch || this.#options.fetch;
    const warn = options.onImageError || this.#options.onImageError || ((src, reason) => console.warn(`pdf-engine: skipped image ${src.slice(0, 100)}: ${reason}`));
    const names = [];
    const bytes = [];
    try {
      await Promise.all(
        sources.slice(0, MAX_IMAGES).map(async (src) => {
          try {
            const found = await loadImage(src, options.images, fetchImpl, options.allowLocalFiles ?? this.#options.allowLocalFiles);
            if (found) {
              names.push(src);
              bytes.push(found);
            }
          } catch (e) {
            warn(src, e?.message || String(e));
          }
        }),
      );
    } catch (e) {
      this.#engine.discard(id);
      throw e;
    }
    const fontNames = [];
    const fontBytes = [];
    await Promise.all(
      fontSources.slice(0, MAX_FONT_FACES).map(async (src) => {
        try {
          const found = await loadFont(src, fetchImpl, options.allowLocalFiles ?? this.#options.allowLocalFiles);
          if (found) {
            fontNames.push(src);
            fontBytes.push(found);
          }
        } catch (e) {
          warn(src, `font: ${e?.message || String(e)}`);
        }
      }),
    );
    return this.#engine.render_prepared(id, names, bytes, fontNames, fontBytes);
  }

  /** Render on a deployed pdf-engine server (`POST {url}/api/to-bytes`). */
  /** The partial templates for one document: the constructor's, then this call's. */
  #partials(options) {
    const merged = { ...this.#options.partials, ...options.partials };
    return Object.keys(merged).length ? merged : undefined;
  }

  async #generateRemote(url, token, { template, data, password, width, height, orientation, partials }) {
    const form = new FormData();
    form.append('template', template ?? '');
    form.append('data', JSON.stringify(data === undefined ? {} : data));
    for (const [name, value] of Object.entries({ width, height, orientation, password })) {
      if (value) form.append(name, value);
    }
    const allPartials = this.#partials({ partials });
    if (allPartials) form.append('partials', JSON.stringify(allPartials));
    const headers = token ? { Authorization: /^Bearer /i.test(token) ? token : `Bearer ${token}` } : {};
    const endpoint = `${String(url).replace(/\/+$/, '')}/api/to-bytes`;
    let response;
    try {
      response = await (this.#options.fetch || fetch)(endpoint, { method: 'POST', body: form, headers });
    } catch (e) {
      throw clientError(
        'SERVER_UNREACHABLE',
        `Could not reach the pdf-engine server at ${endpoint}: ${e?.message || e}`,
        'Check the URL, and that the server allows this origin (CORS_ALLOWED_ORIGINS).',
      );
    }
    if (!response.ok) {
      const body = await response.json().catch(() => null);
      if (body && body.code) throw new PDFEngineError({ ...body, message: body.error });
      throw new PDFEngineError({ code: 'INTERNAL_ERROR', message: `The server answered ${response.status} ${response.statusText}.`, status: response.status });
    }
    return new Uint8Array(await response.arrayBuffer());
  }

  /** Render, then save the PDF through the browser's download (a link with `download`). Browser only. */
  async download(options, filename = 'document.pdf') {
    const url = browserBlobUrl(await this.generate(options));
    const link = document.createElement('a');
    link.href = url;
    link.download = filename;
    document.body.appendChild(link);
    link.click();
    link.remove();
    setTimeout(() => URL.revokeObjectURL(url), 60_000);
  }

  /** Render, then show the PDF in a new tab. Browser only. Resolves with the opened window (or null if blocked). */
  async open(options) {
    const url = browserBlobUrl(await this.generate(options));
    const win = window.open(url, '_blank');
    setTimeout(() => URL.revokeObjectURL(url), 60_000);
    return win;
  }

  /** Render, then open the browser's print dialog for it. Browser only. Printing on thermal or label printers is in the Node package. */
  async print(options) {
    const url = browserBlobUrl(await this.generate(options));
    const frame = document.createElement('iframe');
    frame.style.cssText = 'position:fixed;right:0;bottom:0;width:0;height:0;border:0';
    frame.src = url;
    await new Promise((ok, fail) => {
      frame.onload = ok;
      frame.onerror = () => fail(clientError('BROWSER_PRINT_FAILED', 'The browser could not load the PDF for printing.'));
      document.body.appendChild(frame);
    });
    frame.contentWindow.focus();
    frame.contentWindow.print();
    setTimeout(() => {
      frame.remove();
      URL.revokeObjectURL(url);
    }, 120_000);
  }

  /**
   * Show the document in the page while you build the template. Returns a controller: call `update(options)` after every
   * edit to redraw it (renders are queued, only the latest one runs), `close()` to remove it. Browser only.
   *
   * With `outlines` (the default) every element gets a thin red outline, so you see where boxes, flex rows, table
   * cells and margins really are. The outlines do not change the layout. The PDF viewer shows where pages end.
   *
   * @param {object} options what `generate()` takes
   * @param {{ target?: string|Element, outlines?: boolean|string, height?: string }} [view]
   *        `target`: a selector or element to put the preview in (default: a floating panel).
   */
  async preview(options, view = {}) {
    if (typeof document === 'undefined' || typeof URL === 'undefined' || !URL.createObjectURL) {
      throw clientError('NOT_IN_BROWSER', 'preview() needs a browser page.');
    }
    const { target, outlines = true, height = '80vh' } = view;
    let panel = null;
    let host = typeof target === 'string' ? document.querySelector(target) : target;
    if (target && !host) throw clientError('INVALID_FIELD', `preview(): nothing matches the target ${JSON.stringify(target)}.`);
    if (!host) {
      host = panel = document.createElement('div');
      panel.style.cssText = 'position:fixed;right:12px;bottom:12px;width:min(560px,92vw);z-index:2147483647;background:#fff;border:1px solid #888;border-radius:8px;box-shadow:0 8px 30px rgba(0,0,0,.25);overflow:hidden;font:12px system-ui,sans-serif';
      const bar = document.createElement('div');
      bar.style.cssText = 'display:flex;justify-content:space-between;align-items:center;padding:6px 10px;background:#eef2ff';
      bar.append(Object.assign(document.createElement('span'), { textContent: 'pdf-engine preview' }));
      const x = Object.assign(document.createElement('button'), { textContent: '×', title: 'Close', ariaLabel: 'Close the preview' });
      x.style.cssText = 'border:0;background:none;font-size:18px;cursor:pointer';
      x.onclick = () => controller.close();
      bar.append(x);
      panel.append(bar);
      document.body.append(panel);
    }
    const frame = document.createElement('iframe');
    frame.title = 'PDF preview';
    frame.style.cssText = `width:100%;height:${panel ? '70vh' : height};border:0;background:#f3f4f6`;
    host.append(frame);

    const colour = typeof outlines === 'string' ? outlines : 'rgba(220,38,38,.7)';
    const line = `linear-gradient(${colour},${colour})`;
    // Backgrounds, not `outline` or borders: they are drawn, and they never move anything.
    const debug = `<style>*{background-image:${line},${line},${line},${line};background-size:100% 1px,100% 1px,1px 100%,1px 100%;background-position:top left,bottom left,top left,top right;background-repeat:no-repeat}</style>`;

    let current = options;
    let url = null;
    let running = null;
    let again = false;
    let closed = false;
    const draw = async () => {
      const shown = outlines && typeof current.template === 'string' ? { ...current, template: debug + current.template } : current;
      const bytes = await this.generate(shown);
      if (closed) return;
      const next = URL.createObjectURL(new Blob([bytes], { type: 'application/pdf' }));
      frame.src = `${next}#toolbar=0&navpanes=0&view=FitH`;
      if (url) setTimeout(URL.revokeObjectURL, 10_000, url);
      url = next;
    };
    const controller = {
      element: frame,
      get url() {
        return url;
      },
      /** Redraw, with new options if you pass them. Resolves when the picture is up to date (or rejects with the error). */
      update(next) {
        if (closed) return Promise.resolve();
        if (next) current = next;
        if (running) {
          again = true;
          return running;
        }
        running = (async () => {
          try {
            do {
              again = false;
              await draw();
            } while (again && !closed);
          } finally {
            running = null;
          }
        })();
        return running;
      },
      close() {
        closed = true;
        frame.remove();
        panel?.remove();
        if (url) URL.revokeObjectURL(url);
      },
    };
    await controller.update();
    return controller;
  }

  /** Load the engine now instead of on the first `generate()`. Returns the engine version. */
  async ping() {
    if (this.#workerCount() > 0) return this.#workerPool(this.#workerCount()).ping();
    await this.#start();
    return { version: engineVersion() };
  }

  /** Use these fonts (in addition to the default ones, unless `defaultFonts: false`) from now on. */
  async loadFonts(fonts) {
    this.#options.fonts = [...fonts].filter((f) => f !== '' && f != null);
    if (this.#workerThreads) return this.#workerThreads.loadFonts(this.#options.fonts.map((f) => this.#absolute(f)));
    this.#starting = null;
    await this.#start();
  }

  /** Release the engine's memory. */
  close() {
    this.#closed = true;
    this.#workerThreads?.close();
    this.#workerThreads = null;
    this.#engine?.free();
    this.#engine = null;
    if (activeFonts === this) activeFonts = null;
  }

  async #start() {
    if (this.#closed) throw clientError('ENGINE_CLOSED', 'This PDFEngine was closed; create a new one.');
    if (!this.#starting) {
      this.#starting = (async () => {
        await ensureWasm(this.#options.wasm, this.#options.fetch);
        const sources = [...(this.#options.defaultFonts ? defaultFontUrls() : []), ...this.#options.fonts];
        try {
          this.#fontBytes = await Promise.all(sources.map((s) => loadBytes(s, this.#options.fetch)));
        } catch (e) {
          throw clientError('FONT_LOAD_FAILED', `Could not load a font: ${e?.message || e}`, 'Check the font URLs, or pass the font bytes directly.');
        }
        this.#engine?.free();
        this.#engine = new Engine();
        for (const bytes of this.#fontBytes) this.#engine.add_font(bytes);
        activeFonts = null; // re-apply
      })().catch((e) => {
        this.#starting = null;
        throw e;
      });
    }
    return this.#starting;
  }

  #applyFonts() {
    if (activeFonts !== this) {
      this.#engine.apply_fonts();
      activeFonts = this;
    }
  }
}
