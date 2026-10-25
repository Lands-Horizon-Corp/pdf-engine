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
  | 'INTERNAL_ERROR';

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

export class PDFEngine {
  constructor(options?: PDFEngineOptions | string[]);

  /** Render HTML to a PDF. The engine starts on first use. */
  generate(options: GenerateOptions): Promise<Buffer>;

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
