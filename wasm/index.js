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
import init, { Engine, version as engineVersion } from './dist/pdf_engine_wasm.js';

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
async function loadBytes(source, fetchImpl) {
  if (source instanceof Uint8Array) return source;
  if (source instanceof ArrayBuffer) return new Uint8Array(source);
  if (ArrayBuffer.isView(source)) return new Uint8Array(source.buffer, source.byteOffset, source.byteLength);
  if (typeof Blob !== 'undefined' && source instanceof Blob) return new Uint8Array(await source.arrayBuffer());

  const key = String(source);
  if (!bytesCache.has(key)) {
    bytesCache.set(
      key,
      (async () => {
        if (isNode && (source instanceof URL ? source.protocol === 'file:' : !/^[a-z][a-z0-9+.-]*:/i.test(key) || key.startsWith('file:'))) {
          const { readFile } = await nodeImport('node:fs/promises');
          const { fileURLToPath } = await nodeImport('node:url');
          return new Uint8Array(await readFile(source instanceof URL || key.startsWith('file:') ? fileURLToPath(source) : key));
        }
        const response = await (fetchImpl || fetch)(source);
        if (!response.ok) throw new Error(`${response.status} ${response.statusText} for ${key}`);
        return new Uint8Array(await response.arrayBuffer());
      })(),
    );
    bytesCache.get(key).catch(() => bytesCache.delete(key));
  }
  return bytesCache.get(key);
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
   *           wasm?: string|URL|ArrayBuffer|Uint8Array, fetch?: typeof fetch }} [options]
   */
  constructor(options = {}) {
    if (Array.isArray(options)) options = { fonts: options };
    this.#options = { defaultFonts: true, ...options, fonts: (options.fonts || []).filter((f) => f !== '' && f != null) };
  }

  /** Render HTML to a PDF. Resolves with the PDF bytes. */
  async generate(options) {
    if (!options || typeof options !== 'object') {
      throw clientError('MISSING_FIELD', 'generate() needs an options object with a `template`.');
    }
    const { template, data, password, width, height, orientation } = options;
    await this.#start();
    this.#applyFonts();
    try {
      return this.#engine.generate(
        JSON.stringify({ template, data: data === undefined ? {} : data, password: password || undefined, width, height, orientation }),
      );
    } catch (e) {
      if (typeof WebAssembly !== 'undefined' && e instanceof WebAssembly.RuntimeError) {
        // A trap (a bug in the engine) leaves the module unusable: start over on the next call.
        this.#engine = null;
        this.#starting = null;
        wasmReady = null;
        activeFonts = null;
        throw new PDFEngineError({ code: 'RENDER_FAILED', message: 'The renderer crashed; it will restart on the next call.', status: 500 });
      }
      throw toError(e);
    }
  }

  /** Load the engine now instead of on the first `generate()`. Returns the engine version. */
  async ping() {
    await this.#start();
    return { version: engineVersion() };
  }

  /** Use these fonts (in addition to the default ones, unless `defaultFonts: false`) from now on. */
  async loadFonts(fonts) {
    this.#options.fonts = [...fonts].filter((f) => f !== '' && f != null);
    this.#starting = null;
    await this.#start();
  }

  /** Release the engine's memory. */
  close() {
    this.#closed = true;
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
