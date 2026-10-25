import type { GenerateOptions, PDFEngineError, PDFEngineOptions, PreviewController, PreviewOptions } from './index';

export interface PDFEngineState {
  /** True while a document is being made. */
  busy: boolean;
  /** The last failure, or null. */
  error: PDFEngineError | null;
}

export interface PDFEngineStore {
  /** The Svelte store contract: use it as `$pdf` in a component. */
  subscribe(run: (state: PDFEngineState) => void): () => void;
  generate<T = unknown>(options: GenerateOptions<T>): Promise<Uint8Array>;
  download<T = unknown>(options: GenerateOptions<T>, filename?: string): Promise<void>;
  open<T = unknown>(options: GenerateOptions<T>): Promise<Window | null>;
  print<T = unknown>(options: GenerateOptions<T>): Promise<void>;
  preview<T = unknown>(options: GenerateOptions<T>, view?: PreviewOptions): Promise<PreviewController<T>>;
  ping(): Promise<{ version: string }>;
  clearError(): void;
  /** Release the engine (call it in `onDestroy`). */
  close(): void;
}

/** A pdf-engine as a Svelte store. */
export function createPDFEngine(options?: PDFEngineOptions): PDFEngineStore;
