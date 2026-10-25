import type { GenerateOptions, PDFEngineError, PDFEngineOptions, PreviewController, PreviewOptions } from './index';

export interface UsePDFEngine {
  /** Make a PDF: resolves with its bytes. */
  generate<T = unknown>(options: GenerateOptions<T>): Promise<Uint8Array>;
  /** Make a PDF and save it through the browser's download. */
  download<T = unknown>(options: GenerateOptions<T>, filename?: string): Promise<void>;
  /** Make a PDF and show it in a new tab. */
  open<T = unknown>(options: GenerateOptions<T>): Promise<Window | null>;
  /** Make a PDF and open the browser's print dialog. */
  print<T = unknown>(options: GenerateOptions<T>): Promise<void>;
  /** Show a live preview. */
  preview<T = unknown>(options: GenerateOptions<T>, view?: PreviewOptions): Promise<PreviewController<T>>;
  /** Load the engine now, to hide the first-call delay. */
  ping(): Promise<{ version: string }>;
  /** True while a document is being made. */
  busy: import('vue').Ref<boolean>;
  /** The last failure, or null. */
  error: import('vue').Ref<PDFEngineError | null>;
  clearError(): void;
}

/** A pdf-engine for this component or scope: made on first use, closed with it. */
export function usePDFEngine(options?: PDFEngineOptions): UsePDFEngine;
