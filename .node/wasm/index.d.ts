/** Error codes the engine sends. Must match `AppError::CODES` in src/error.rs (a Rust test checks). */
export type ErrorCode =
  | 'UNAUTHORIZED'
  | 'MISSING_FIELD'
  | 'INVALID_FIELD'
  | 'IPC_PROTOCOL_ERROR'
  | 'FONT_ERROR'
  | 'PAYLOAD_TOO_LARGE'
  | 'TEMPLATE_ERROR'
  | 'TEMPLATE_OUTPUT_TOO_LARGE'
  | 'TEMPLATE_TOO_EXPENSIVE'
  | 'IO_ERROR'
  | 'STORAGE_ERROR'
  | 'STORAGE_NOT_CONFIGURED'
  | 'RENDER_FAILED'
  | 'PDF_PROCESSING_FAILED'
  | 'TIMEOUT'
  | 'INTERNAL_ERROR'
  | 'PRINT_FAILED'
  | 'PRINTER_UNREACHABLE';

/** Error codes produced by this package, not the engine. */
export type ClientErrorCode = 'WASM_LOAD_FAILED' | 'FONT_LOAD_FAILED' | 'ENGINE_CLOSED' | 'SERVER_UNREACHABLE' | 'NOT_IN_BROWSER' | 'BROWSER_PRINT_FAILED';

export interface PDFEngineErrorInit {
  code: ErrorCode | ClientErrorCode;
  message: string;
  status?: number;
  hint?: string;
  line?: number;
}

export class PDFEngineError extends Error {
  constructor(init: PDFEngineErrorInit);
  readonly name: 'PDFEngineError';
  /** Stable identifier to branch on. */
  readonly code: ErrorCode | ClientErrorCode;
  /** HTTP-style status (400 for bad input, 500 for engine failures). Engine errors only. */
  readonly status?: number;
  /** What to do about it. */
  readonly hint?: string;
  /** 1-based template line, for `TEMPLATE_ERROR`. */
  readonly line?: number;
}

/** A font as a URL (fetched once and cached), bytes, or a Blob/File. */
export type FontSource = string | URL | ArrayBuffer | Uint8Array | Blob;

export interface PDFEngineOptions {
  /**
   * Make documents in this many Web Workers (Worker threads under Node) instead of on the page's own thread, so
   * several are made at once and the page stays smooth. `'auto'` is the cores minus one, at most 4. Each worker loads
   * its own copy of the engine (about 10 MB). Not used for documents sent to a `url`. Default: 0, this thread.
   */
  workers?: number | 'auto';
  /** Partial templates every document can include (see `GenerateOptions.partials`). */
  partials?: Record<string, string>;
  /**
   * Fonts to add (.ttf, .otf, .ttc). A browser has no system fonts, so any script other than
   * Latin, Cyrillic, Greek and Vietnamese needs one, for example a CJK or Arabic font.
   */
  fonts?: FontSource[];
  /** Load the bundled Noto Sans (Regular and Bold). Default `true`. */
  defaultFonts?: boolean;
  /** Where to load the `.wasm` file from, if your bundler does not find it automatically. */
  wasm?: string | URL | ArrayBuffer | Uint8Array;
  /** `fetch` used for fonts, the wasm file and server requests (default: the global one). */
  fetch?: typeof fetch;
  /**
   * A deployed pdf-engine server (for example `https://pdf.example.com`). When set, documents are
   * rendered there instead of in the page, and no WebAssembly is loaded. The server must allow your
   * origin (`CORS_ALLOWED_ORIGINS`).
   */
  url?: string;
  /** Default for `GenerateOptions.compression`. */
  compression?: 'fast' | 'balanced' | 'small';
  /** Default for `GenerateOptions.onImageError`. */
  onImageError?: (src: string, reason: string) => void;
  /** Default for `GenerateOptions.allowLocalFiles` (Node only). */
  allowLocalFiles?: boolean;
  /** The server's `API_BEARER_TOKEN`. Anything in browser code is visible to users: prefer a proxy or a short-lived token. */
  token?: string | string[];
}

export type Pick = 'server' | 'browser' | 'auto';

export interface GenerateOptions<T = unknown> {
  /** HTML with `{{ value }}` / `{% for x in list %}` placeholders (minijinja, auto-escaped). */
  template: string;
  /** Values for the placeholders. Pass your own interface as `generate<MyData>()` to get it checked. */
  data?: T;
  /** Encrypts the PDF (open and owner password). Empty or missing = not encrypted. */
  password?: string;
  /**
   * Make a tagged, accessible PDF (PDF/UA): a structure tree, the document language (`<html lang>`, else English)
   * and the title (`<title>`). A page with no headings has no outline, which PDF/UA needs, so it is tagged but not
   * declared PDF/UA.
   */
  accessible?: boolean;
  /**
   * Named templates the template can `{% include "name" %}`, `{% extends "name" %}` or `{% import "name" as x %}`,
   * for headers, footers, rows and layouts. Merged over the constructor's `partials`.
   */
  partials?: Record<string, string>;
  /** Page width, e.g. `8.5in`, `210mm`, `21cm`, `612pt`, `816px` (default `8.5in`). */
  width?: string;
  /** Page height (default `11in`). */
  height?: string;
  /** `landscape` swaps width and height (default `portrait`). */
  orientation?: 'portrait' | 'landscape';
  /**
   * Images for `<img src="name">` tags, by the exact `src` value: bytes, a Blob or File, a URL,
   * or a `data:` URI. Any other `src` (https URL, `blob:`, a relative URL) is fetched as is.
   * `data:` URIs inside the template work without this. An image that cannot be loaded is skipped
   * (see `onImageError`), never fails the document.
   */
  images?: Record<string, Uint8Array | ArrayBuffer | Blob | string | URL>;
  /**
   * How hard to compress the PDF. `fast` (default) stores streams uncompressed: the quickest, and a receipt is
   * about 40 KB instead of 24 KB. `balanced` is light compression, `small` the smallest file and about 20% slower.
   */
  compression?: 'fast' | 'balanced' | 'small';
  /** Called for each image that was skipped, with its `src` and the reason. Default: `console.warn`. */
  onImageError?: (src: string, reason: string) => void;
  /** Node only: let `<img src>` read local files and `file:` URLs. Off by default. */
  allowLocalFiles?: boolean;
  /**
   * pdf-engine servers (the Docker image) for this document, overriding the constructor's `url`. Give
   * several to spread the load: they are used in turn, and one that is down or overloaded is skipped.
   */
  url?: string | string[];
  /** Bearer token: one for every server, or an array that lines up with `url` (overrides the constructor's `token`). */
  token?: string | string[];
  /**
   * Where to render. `'server'` (default when there is a `url`), `'browser'` (WebAssembly in this page; the
   * default without a `url`), or `'auto'`: small documents in the page, documents over `autoMaxBytes` on the
   * server, and the page again when the servers cannot be reached or the browser is offline.
   */
  pick?: Pick;
  /** With `pick: 'auto'`: the template plus data size, in bytes or like `'2mb'`, above which the server is used. */
  autoMaxBytes?: number | string;
  /** Render in the page when every server fails (a network or overload error). On by default only for `pick: 'auto'`. */
  fallback?: boolean;
  /**
   * Send only the `data` keys the template names (on by default), which keeps big payloads and secrets
   * you did not mean to use out of the renderer and off the network. Templates that look keys up
   * dynamically (`self`, `include`) always get everything.
   */
  pruneData?: boolean;
}

export interface PreviewOptions {
  /** A selector or element to put the preview in. Default: a floating panel with a close button. */
  target?: string | Element;
  /** Draw a thin outline around every element (default `true`); a string is the colour. Layout is not changed. */
  outlines?: boolean | string;
  /** Height of the preview (default `80vh`). */
  height?: string;
}

export interface PreviewController<T = unknown> {
  readonly element: HTMLIFrameElement;
  /** The current PDF's blob URL. */
  readonly url: string | null;
  /** Redraw, optionally with new options. Resolves when the picture is current; rejects with the engine's error. */
  update(options?: GenerateOptions<T>): Promise<void>;
  close(): void;
}

export class PDFEngine {
  constructor(options?: PDFEngineOptions | FontSource[]);

  /** Render HTML to a PDF. The engine loads on first use. */
  generate<T = unknown>(options: GenerateOptions<T>): Promise<Uint8Array>;

  /** Render, then save the PDF through the browser's download. Browser only. */
  download<T = unknown>(options: GenerateOptions<T>, filename?: string): Promise<void>;

  /**
   * Show the document in the page while you build the template, with thin outlines around every box. Returns a
   * controller: `update(options)` redraws (queued; only the latest runs), `close()` removes it. Browser only.
   */
  preview<T = unknown>(options: GenerateOptions<T>, view?: PreviewOptions): Promise<PreviewController<T>>;

  /** Render, then show the PDF in a new tab. Browser only. Resolves with the new window, or null if it was blocked. */
  open<T = unknown>(options: GenerateOptions<T>): Promise<Window | null>;

  /** Render, then open the browser's print dialog for it. Browser only. */
  print<T = unknown>(options: GenerateOptions<T>): Promise<void>;

  /**
   * Options that make a PDF of an element as it looks on screen (browser pages only): computed styles copied inline,
   * canvases and images as data, inline SVG, the `@font-face` fonts in use, and a page sized to the element. Pass the
   * result to `generate()`, `download()`, `open()` or `preview()`.
   * @param element an element, or a CSS selector
   * @param options `margin` (px, default 16); `images`: `'inline'` (default) makes data URLs, `'link'` keeps addresses;
   * `fonts`: `false` leaves the `@font-face` rules out. Anything else (`password`, `accessible`, ...) goes to `generate()`.
   */
  fromElement(
    element: Element | string,
    options?: Omit<GenerateOptions, 'template'> & { margin?: number; images?: 'inline' | 'link'; fonts?: boolean },
  ): Promise<GenerateOptions>;

  /** Load the engine now and report its version. */
  ping(): Promise<{ version: string }>;

  /** Use these fonts (besides the default ones) from now on. */
  loadFonts(fonts: FontSource[]): Promise<void>;

  /** Release the engine's memory. */
  close(): void;
}
