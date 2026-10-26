/** Error codes the Rust engine sends. Must match `AppError::CODES` in src/error.rs (a Rust test checks). */
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

/** Error codes produced by this client, not the engine. */
export type ClientErrorCode =
  | 'BINARY_NOT_FOUND'
  | 'ENGINE_START_FAILED'
  | 'ENGINE_EXITED'
  | 'ENGINE_CLOSED'
  | 'CLIENT_TIMEOUT'
  | 'PROTOCOL_MISMATCH';

/** Wire protocol version; the client refuses an engine that reports a different one. */
export const PROTOCOL_VERSION: 1;

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
  /** HTTP-style status (400 for bad input, 408 timeout, 500 for engine failures). Engine errors only. */
  readonly status?: number;
  /** What to do about it. */
  readonly hint?: string;
  /** 1-based template line, for `TEMPLATE_ERROR`. */
  readonly line?: number;
}

export interface PDFEngineOptions {
  /** Partial templates every document can include (see `GenerateOptions.partials`). */
  partials?: Record<string, string>;
  /**
   * Font files (.ttf, .otf, .ttc) or folders of them, added to the system fonts. Needed for
   * fonts the user's machine may not have.
   */
  fonts?: string[];
  /** Path to the `pdf-engine` executable. Found automatically in a normal install. */
  binary?: string;
  /** Give up waiting for one answer after this many ms (default 60000; 0 = never). */
  timeoutMs?: number;
  /**
   * Folder `<img src>` may read local files from: `src="logo.png"` or a `file:` URL inside it.
   * Nothing outside it (`..`, symlinks) is read. Off by default.
   */
  imageDir?: string;
  /** Hosts `<img src="https://...">` may be fetched from (`cdn.example.com`, `*.example.com`). Off by default. */
  imageHosts?: string[];
  /** Extra environment variables for the engine (e.g. `ASSET_ALLOWED_HOSTS`, `RENDER_TIMEOUT_SECS`). */
  env?: Record<string, string>;
  /** Receives the engine's log output (stderr). */
  onLog?: (text: string) => void;
}

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
   * Archival PDF/A output at this level: `'2b'` (the usual choice), `'1b'`, `'2u'`, `'3b'`, or `'2a'` / `'3a'` (also tagged).
   * Fonts are embedded and the file is checked against the standard; a document that cannot meet it is a
   * `RENDER_FAILED` error naming the problem. Cannot be combined with `password` (PDF/A forbids encryption).
   */
  pdfA?: '1b' | '2b' | '2u' | '2a' | '3b' | '3a';
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
   * Images for `<img src="name">`, by the exact `src` value: a Buffer or Uint8Array, a `data:` URI,
   * or the path of a file (read by your program, so it is not limited by `imageDir`). They are
   * sent to the engine with the request. An image that cannot be used is skipped, never an error.
   */
  images?: Record<string, Uint8Array | ArrayBuffer | string>;
}

export interface PingResult {
  /** Engine version. */
  version: string;
  /** Wire protocol version. */
  protocol: number;
}

export type PrinterType = 'thermal' | 'label' | 'document';

export interface Printer {
  /** Pass this as `printerId`: `system://<queue name>`, `net://<host>:<port>` or `usb://<vendor>:<product>`. */
  id: string;
  name: string;
  /** `thermal` = receipt roll (ESC/POS), `label` = label/barcode (ZPL), `document` = office printer (PDF). Guessed from the name; override it in `print()`. */
  type: PrinterType;
  status: 'ready' | 'busy' | 'offline' | 'unknown';
  transport: 'system' | 'net' | 'usb';
  default: boolean;
  /** Where the system says it is connected (`usb://...`, `socket://...`, a Windows port). */
  uri?: string;
}

export interface PrintOptions<T = unknown> extends GenerateOptions<T> {
  /** From `printers()`, or `net://host[:9100]` for a network printer. */
  printerId: string;
  /** Force the kind of output: a receipt printer is sent ESC/POS, a label printer ZPL, an office printer the PDF. */
  type?: PrinterType;
  /** Roll width for `thermal`/`label`: `'58mm'`, `'80mm'` (default) or `'112mm'`, or the printable width in dots (203 dpi). */
  paper?: '58mm' | '80mm' | '112mm' | number;
  /** Raster command language; default `escpos` for `thermal`, `zpl` for `label`. */
  language?: 'escpos' | 'zpl';
  /** Cut the paper after printing (ESC/POS; default `true`). */
  cut?: boolean;
  /** Blank lines to feed before the cut (ESC/POS; default 3). */
  feed?: number;
  /** Spread greys into dots, for photos and logos (default `false`; text is sharper without it). */
  dither?: boolean;
  /** Copies (1 to 100). */
  copies?: number;
}

export interface PrintResult {
  printerId: string;
  type: PrinterType;
  /** What was sent: `escpos`, `zpl` or `pdf`. */
  language: 'escpos' | 'zpl' | 'pdf';
  bytes: number;
  copies: number;
}

export class PDFEngine {
  constructor(options?: PDFEngineOptions | string[]);

  /**
   * The printers this computer can print to: those the operating system lists plus USB receipt and label
   * printers seen directly. Network printers cannot be discovered: print to one with
   * `printerId: 'net://host[:9100]'`.
   */
  printers(): Promise<Printer[]>;

  /** Render the template and print it on `printerId`. */
  print<T = unknown>(options: PrintOptions<T>): Promise<PrintResult>;

  /** Render HTML to a PDF. The engine starts on first use. */
  generate<T = unknown>(options: GenerateOptions<T>): Promise<Buffer>;

  /** The engine's process id while it runs, else `null`. */
  readonly pid: number | null;

  /** Start the engine now and report its version. */
  ping(): Promise<PingResult>;

  /** Use these font files or folders (besides the system fonts) from now on. */
  loadFonts(paths: string[]): Promise<void>;

  /** Pick up fonts installed on or removed from the system. */
  reloadFonts(): Promise<void>;

  /** Finish the requests in progress, then stop the engine. */
  close(): Promise<void>;
}

/** Where the engine binary is looked for, in order. */
export function binaryCandidates(options?: Pick<PDFEngineOptions, 'binary'>): string[];
