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
  | 'INTERNAL_ERROR';

/** Error codes produced by this package, not the engine. */
export type ClientErrorCode = 'WASM_LOAD_FAILED' | 'FONT_LOAD_FAILED' | 'ENGINE_CLOSED';

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
   * Fonts to add (.ttf, .otf, .ttc). A browser has no system fonts, so any script other than
   * Latin, Cyrillic, Greek and Vietnamese needs one, for example a CJK or Arabic font.
   */
  fonts?: FontSource[];
  /** Load the bundled Noto Sans (Regular and Bold). Default `true`. */
  defaultFonts?: boolean;
  /** Where to load the `.wasm` file from, if your bundler does not find it automatically. */
  wasm?: string | URL | ArrayBuffer | Uint8Array;
  /** `fetch` used for fonts and the wasm file (default: the global one). */
  fetch?: typeof fetch;
}

export interface GenerateOptions {
  /** HTML with `{{ value }}` / `{% for x in list %}` placeholders (minijinja, auto-escaped). */
  template: string;
  /** Values for the placeholders. */
  data?: unknown;
  /** Encrypts the PDF (open and owner password). Empty or missing = not encrypted. */
  password?: string;
  /** Page width, e.g. `8.5in`, `210mm`, `21cm`, `612pt`, `816px` (default `8.5in`). */
  width?: string;
  /** Page height (default `11in`). */
  height?: string;
  /** `landscape` swaps width and height (default `portrait`). */
  orientation?: 'portrait' | 'landscape';
}

export class PDFEngine {
  constructor(options?: PDFEngineOptions | FontSource[]);

  /** Render HTML to a PDF. The engine loads on first use. */
  generate(options: GenerateOptions): Promise<Uint8Array>;

  /** Load the engine now and report its version. */
  ping(): Promise<{ version: string }>;

  /** Use these fonts (besides the default ones) from now on. */
  loadFonts(fonts: FontSource[]): Promise<void>;

  /** Release the engine's memory. */
  close(): void;
}
