'use strict';

/**
 * HTML to PDF for Node.js and Electron.
 *
 * Spawns the Rust engine (`pdf-engine ipc`) once and talks to it over stdin/stdout, so there is
 * no HTTP server, no port and no network round trip. The PDF comes back as raw bytes.
 *
 *   const { PDFEngine } = require('@lands-horizon-corp/pdf-engine');
 *   const pdf = new PDFEngine({ fonts: ['/path/to/Inter.ttf'] });
 *   const bytes = await pdf.generate({ template: '<h1>Invoice {{ no }}</h1>', data: { no: 42 } });
 *   await pdf.close();
 *
 * Wire format: see src/ipc.rs in the engine. `PROTOCOL_VERSION` here must equal the engine's.
 */

const { spawn } = require('node:child_process');
const fs = require('node:fs');
const path = require('node:path');

const PROTOCOL_VERSION = 1;

/** An error from the engine (it carries the engine's `code`, `status`, `hint`, `line`) or from this client. */
/** Environment that tells the engine where `<img src>` may come from. */
function imageEnv({ imageDir, imageHosts }) {
  const env = {};
  if (imageDir) env.ASSET_LOCAL_DIR = path.resolve(imageDir);
  if (imageHosts?.length) env.ASSET_ALLOWED_HOSTS = imageHosts.join(',');
  return env;
}

/** The `images` option as bytes: Buffers and typed arrays as they are, a string as a `data:` URI or a file path. */
async function collectImages(images) {
  const out = [];
  for (const [src, value] of Object.entries(images)) {
    let bytes;
    if (typeof value === 'string') {
      bytes = value.startsWith('data:')
        ? decodeDataUri(value)
        : await fs.promises.readFile(value).catch((e) => {
            throw clientError('INVALID_FIELD', `images["${src}"]: cannot read ${value}: ${e.code || e.message}`);
          });
    } else if (value instanceof ArrayBuffer) {
      bytes = Buffer.from(value);
    } else if (ArrayBuffer.isView(value)) {
      bytes = Buffer.from(value.buffer, value.byteOffset, value.byteLength);
    } else {
      throw clientError('INVALID_FIELD', `images["${src}"] must be a Buffer, a Uint8Array, a data: URI or a file path.`);
    }
    out.push({ src, bytes });
  }
  return out;
}

function decodeDataUri(uri) {
  const comma = uri.indexOf(',');
  if (comma < 0) throw clientError('INVALID_FIELD', 'images: malformed data: URI.');
  return /;base64$/i.test(uri.slice(0, comma)) ? Buffer.from(uri.slice(comma + 1), 'base64') : Buffer.from(decodeURIComponent(uri.slice(comma + 1)));
}

class PDFEngineError extends Error {
  constructor({ code, message, status, hint, line }) {
    super(message);
    this.name = 'PDFEngineError';
    this.code = code;
    if (status !== undefined) this.status = status;
    if (hint !== undefined) this.hint = hint;
    if (line !== undefined) this.line = line;
  }
}

function clientError(code, message, hint) {
  return new PDFEngineError({ code, message, hint });
}

const exeName = () => (process.platform === 'win32' ? 'pdf-engine.exe' : 'pdf-engine');
const platformDir = () => `${process.platform}-${process.arch}`;
/** Executables cannot run from inside an Electron asar archive; use the unpacked copy. */
const unpacked = (p) => p.replace(/app\.asar(?=[\\/])/, 'app.asar.unpacked');

/** Where the engine binary is looked for, in order. */
function binaryCandidates(options = {}) {
  const list = [];
  if (options.binary) list.push(options.binary);
  if (process.env.PDF_ENGINE_BIN) list.push(process.env.PDF_ENGINE_BIN);
  list.push(unpacked(path.join(__dirname, 'bin', platformDir(), exeName())));
  // Electron: electron-builder `extraResources` lands in process.resourcesPath.
  if (process.resourcesPath) {
    list.push(path.join(process.resourcesPath, 'pdf-engine', platformDir(), exeName()));
    list.push(path.join(process.resourcesPath, 'bin', exeName()));
  }
  return list;
}

class PDFEngine {
  #options;
  #child = null;
  #starting = null;
  #ready = false;
  #closed = false;
  #nextId = 1;
  #pending = new Map();
  #chunks = [];
  #size = 0;
  #stderrTail = '';
  #fatal = null;

  /**
   * @param {{ fonts?: string[], binary?: string, timeoutMs?: number, env?: Record<string,string>,
   *           imageDir?: string, imageHosts?: string[],
   *           onLog?: (text: string) => void } | string[]} [options]
   */
  constructor(options = {}) {
    if (Array.isArray(options)) options = { fonts: options };
    this.#options = {
      timeoutMs: 60_000,
      env: {},
      ...options,
      fonts: (options.fonts || []).filter((f) => typeof f === 'string' && f.trim() !== ''),
    };
  }

  /** Render HTML to a PDF. Resolves with the PDF bytes. */
  async generate(options) {
    if (!options || typeof options !== 'object') {
      throw clientError('MISSING_FIELD', 'generate() needs an options object with a `template`.');
    }
    const { template, data, password, width, height, orientation, accessible, pdfA } = options;
    const supplied = options.images ? await collectImages(options.images) : [];
    await this.#ensureStarted();
    const { body } = await this.#request(
      {
        op: 'render',
        template,
        data: data === undefined ? {} : data,
        password: password || undefined,
        accessible: accessible || undefined,
        pdf_a: pdfA || undefined,
        width,
        height,
        orientation,
        partials: this.#partials(options),
        images: supplied.length ? supplied.map(({ src, bytes }) => ({ src, len: bytes.length })) : undefined,
      },
      this.#options.timeoutMs,
      supplied.length ? Buffer.concat(supplied.map((i) => i.bytes)) : null,
    );
    return body;
  }

  /**
   * The printers this computer can print to: those the operating system knows (USB, network and Bluetooth
   * printers with their driver installed) and USB receipt and label printers the engine sees directly.
   * A network printer cannot be discovered: print to it with `printerId: 'net://host[:9100]'`.
   */
  async printers() {
    await this.#ensureStarted();
    const { header } = await this.#request({ op: 'printers' });
    return header.printers;
  }

  /**
   * Render a template and print it. A receipt printer gets ESC/POS, a label printer ZPL, an office printer
   * the PDF. See `printers()` for the ids.
   */
  async print(options) {
    if (!options || typeof options !== 'object') {
      throw clientError('MISSING_FIELD', 'print() needs an options object with a `printerId` and a `template`.');
    }
    const { printerId, type, paper, language, cut, feed, dither, copies, template, data, width, height, orientation } = options;
    const supplied = options.images ? await collectImages(options.images) : [];
    await this.#ensureStarted();
    const { header } = await this.#request(
      {
        op: 'print',
        printerId,
        type,
        paper,
        language,
        cut,
        feed,
        dither,
        copies,
        template,
        data: data === undefined ? {} : data,
        width,
        height,
        orientation,
        partials: this.#partials(options),
        images: supplied.length ? supplied.map(({ src, bytes }) => ({ src, len: bytes.length })) : undefined,
      },
      this.#options.timeoutMs,
      supplied.length ? Buffer.concat(supplied.map((i) => i.bytes)) : null,
    );
    return { printerId: header.printerId, type: header.type, language: header.language, bytes: header.bytes, copies: header.copies };
  }

  /** The engine's process id while it runs (for monitoring, or to stop it from outside), else `null`. */
  get pid() {
    return this.#child?.pid ?? null;
  }

  /** The partial templates for one document: the constructor's, then this call's. */
  #partials(options) {
    const merged = { ...this.#options.partials, ...options.partials };
    return Object.keys(merged).length ? merged : undefined;
  }

  /** Start the engine now instead of on the first `generate()`. Returns its version information. */
  async ping() {
    await this.#ensureStarted();
    const { header } = await this.#request({ op: 'ping' });
    return { version: header.version, protocol: header.protocol };
  }

  /** Use these font files or folders (in addition to the system fonts) from now on. */
  async loadFonts(paths) {
    const fonts = [...paths].filter((f) => typeof f === 'string' && f.trim() !== '');
    const previous = this.#options.fonts;
    // The engine reads these again whenever it restarts: a list that failed to load must not be kept.
    this.#options.fonts = fonts;
    try {
      await this.#ensureStarted();
      await this.#request({ op: 'load_fonts', paths: fonts.map((p) => path.resolve(p)) });
    } catch (e) {
      this.#options.fonts = previous;
      throw e;
    }
  }

  /** Pick up fonts that were installed or removed on the system. */
  async reloadFonts() {
    await this.#ensureStarted();
    await this.#request({ op: 'reload_fonts' });
  }

  /** Finish the requests in progress, then stop the engine. */
  async close() {
    this.#closed = true;
    const child = this.#child;
    if (!child) return;
    await new Promise((resolve) => {
      const kill = setTimeout(() => child.kill(), 30_000);
      child.once('exit', () => {
        clearTimeout(kill);
        resolve();
      });
      child.stdin.end();
    });
  }

  // ---- internals -------------------------------------------------------------------------

  #ensureStarted() {
    if (this.#closed) {
      return Promise.reject(clientError('ENGINE_CLOSED', 'This PDFEngine was closed; create a new one.'));
    }
    if (this.#ready && this.#child) return Promise.resolve();
    if (!this.#starting) {
      this.#starting = this.#start().catch((e) => {
        this.#starting = null;
        throw e;
      });
    }
    return this.#starting;
  }

  async #start() {
    const tried = binaryCandidates(this.#options);
    const binary = tried.find((p) => fs.existsSync(p)) || exeName();
    const child = spawn(binary, ['ipc'], {
      stdio: ['pipe', 'pipe', 'pipe'],
      env: { ...process.env, ...imageEnv(this.#options), ...this.#options.env },
      windowsHide: true,
    });
    this.#child = child;
    this.#chunks = [];
    this.#size = 0;
    this.#fatal = null;

    child.on('error', (e) => {
      const missing = e.code === 'ENOENT';
      this.#failAll(
        missing
          ? clientError(
              'BINARY_NOT_FOUND',
              `The pdf-engine binary was not found. Looked in: ${tried.join(', ')}, then PATH.`,
              'Run `npm run build:native` in the pdf-engine package, or pass { binary: "/path/to/pdf-engine" }.',
            )
          : clientError('ENGINE_START_FAILED', `Could not start pdf-engine: ${e.message}`),
      );
    });
    child.on('exit', (code, signal) => this.#onExit(child, code, signal));
    child.stdout.on('data', (chunk) => this.#onData(chunk));
    child.stderr.on('data', (chunk) => {
      const text = chunk.toString();
      this.#stderrTail = (this.#stderrTail + text).slice(-4000);
      if (this.#options.onLog) this.#options.onLog(text);
    });
    child.stdin.on('error', () => {}); // a dead engine is reported through 'exit'

    try {
      const hello = await this.#request({ op: 'ping' }, 30_000);
      if (hello.header.protocol !== PROTOCOL_VERSION) {
        throw clientError(
          'PROTOCOL_MISMATCH',
          `The engine speaks protocol ${hello.header.protocol} but this client speaks ${PROTOCOL_VERSION}.`,
          'Install matching versions of the pdf-engine package and its binary.',
        );
      }
      if (this.#options.fonts.length) {
        await this.#request({ op: 'load_fonts', paths: this.#options.fonts.map((p) => path.resolve(p)) }, 60_000);
      }
    } catch (e) {
      // A start that fails must not leave its process running (and lost): the next call would start another.
      if (this.#child === child) {
        this.#child = null;
        this.#ready = false;
      }
      child.kill();
      throw e;
    }
    this.#ready = true;
  }

  #onExit(child, code, signal) {
    if (this.#child !== child) return;
    this.#child = null;
    this.#ready = false;
    this.#starting = null;
    if (this.#closed && this.#pending.size === 0) return;
    const detail = this.#fatal
      ? this.#fatal.error
      : this.#stderrTail.trim() || 'no output';
    this.#failAll(clientError('ENGINE_EXITED', `pdf-engine stopped unexpectedly (exit code ${code ?? signal}): ${detail}`));
  }

  #failAll(error) {
    for (const [id, p] of this.#pending) {
      clearTimeout(p.timer);
      this.#pending.delete(id);
      p.reject(error);
    }
  }

  async #request(header, timeoutMs = this.#options.timeoutMs, payload = null) {
    const child = this.#child;
    if (!child) throw clientError('ENGINE_EXITED', 'The engine is not running.');
    const id = this.#nextId++;
    const json = Buffer.from(JSON.stringify({ id, ...header }));
    const frame = Buffer.allocUnsafe(8 + json.length + (payload ? payload.length : 0));
    frame.writeUInt32BE(json.length, 0);
    frame.writeUInt32BE(payload ? payload.length : 0, 4);
    json.copy(frame, 8);
    if (payload) payload.copy(frame, 8 + json.length);

    const reply = new Promise((resolve, reject) => {
      const timer =
        timeoutMs > 0
          ? setTimeout(() => {
              this.#pending.delete(id);
              reject(clientError('CLIENT_TIMEOUT', `No answer from pdf-engine within ${timeoutMs} ms.`));
            }, timeoutMs)
          : null;
      this.#pending.set(id, { resolve, reject, timer });
    });
    // The engine can fail while the frame is still being written; the caller gets the error
    // when it awaits `reply` below, so this is not an unhandled rejection.
    reply.catch(() => {});
    await new Promise((resolve) => {
      const stdin = child.stdin;
      if (stdin.write(frame)) return resolve();
      const done = () => {
        stdin.off('drain', done);
        stdin.off('close', done);
        resolve();
      };
      stdin.once('drain', done);
      stdin.once('close', done);
    });
    return reply;
  }

  #onData(chunk) {
    this.#chunks.push(chunk);
    this.#size += chunk.length;
    for (;;) {
      if (this.#size < 8) return;
      const head = this.#chunks[0].length >= 8 ? this.#chunks[0] : Buffer.concat(this.#chunks);
      const headerLen = head.readUInt32BE(0);
      const bodyLen = head.readUInt32BE(4);
      const total = 8 + headerLen + bodyLen;
      if (this.#size < total) return;

      const all = this.#chunks.length === 1 ? this.#chunks[0] : Buffer.concat(this.#chunks);
      const rest = all.subarray(total);
      this.#chunks = rest.length ? [rest] : [];
      this.#size = rest.length;
      const header = JSON.parse(all.toString('utf8', 8, 8 + headerLen));
      // Copy the body when it would otherwise keep the following frames' bytes alive.
      const body = rest.length ? Buffer.from(all.subarray(8 + headerLen, total)) : all.subarray(8 + headerLen, total);
      this.#dispatch(header, body);
    }
  }

  #dispatch(header, body) {
    if (header.id === null || header.id === undefined) {
      // A fatal stream error: the engine reports it, then exits.
      if (header.ok === false) this.#fatal = header.error;
      return;
    }
    const pending = this.#pending.get(header.id);
    if (!pending) return; // answered after its timeout
    this.#pending.delete(header.id);
    clearTimeout(pending.timer);
    if (header.ok) pending.resolve({ header, body });
    else pending.reject(new PDFEngineError({ ...header.error, message: header.error.error }));
  }
}

module.exports = { PDFEngine, PDFEngineError, PROTOCOL_VERSION, binaryCandidates };
