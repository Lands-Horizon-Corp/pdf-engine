/**
 * What every worker does, whatever runs it (a Web Worker or a Node worker thread). `worker.js` and `worker-node.js`
 * are the two entry points; you do not use these files yourself.
 *
 * Messages in:  { type: 'init', options } then { type: 'generate' | 'ping' | 'loadFonts', id, ... }
 * Messages out: { id, ok: true, bytes | version } or { id, ok: false, error }, and { type: 'imageError', id, src, reason }
 */
import { PDFEngine, PDFEngineError } from './index.js';

/** @param {(message: object, transfer?: Transferable[]) => void} post  @param {(handler: (message: object) => void) => void} listen */
export function serve(post, listen) {
  let engine = null;

  const failure = (id, e) => {
    const error = e instanceof PDFEngineError ? { code: e.code, message: e.message, status: e.status, hint: e.hint, line: e.line } : { code: 'INTERNAL_ERROR', message: e?.message || String(e), status: 500 };
    post({ id, ok: false, error });
  };

  listen(async (message) => {
    const { type, id } = message;
    try {
      if (type === 'init') {
        engine = new PDFEngine(message.options);
        return;
      }
      if (type === 'generate') {
        const bytes = await engine.generate({ ...message.options, onImageError: (src, reason) => post({ type: 'imageError', id, src, reason }) });
        // Hand the buffer over instead of copying it.
        const owned = bytes.byteOffset === 0 && bytes.byteLength === bytes.buffer.byteLength ? bytes : bytes.slice();
        post({ id, ok: true, bytes: owned }, [owned.buffer]);
      } else if (type === 'ping') {
        post({ id, ok: true, ...(await engine.ping()) });
      } else if (type === 'loadFonts') {
        await engine.loadFonts(message.fonts);
        post({ id, ok: true });
      }
    } catch (e) {
      failure(id, e);
    }
  });
}
