/**
 * Svelte store for pdf-engine in the browser (Svelte 3, 4 and 5: it is a plain store).
 *
 *   <script>
 *     import { createPDFEngine } from '@lands-horizon-corp/pdf-engine-wasm/svelte';
 *     const pdf = createPDFEngine();
 *     import { onDestroy } from 'svelte';
 *     onDestroy(pdf.close);
 *   </script>
 *   <button disabled={$pdf.busy} on:click={() => pdf.download({ template, data }, 'invoice.pdf')}>Download</button>
 *
 * `$pdf.busy` is true while a document is being made and `$pdf.error` is the last failure or null. The methods
 * still reject, so you can also `try`/`catch`. Call `close()` when you are done (`onDestroy`).
 */
import { PDFEngine } from './index.js';

export function createPDFEngine(options) {
  let engine = null;
  let running = 0;
  let state = { busy: false, error: null };
  const subscribers = new Set();
  const set = (next) => {
    state = { ...state, ...next };
    for (const run of subscribers) run(state);
  };
  const get = () => (engine ??= new PDFEngine(options));

  const wrap =
    (method) =>
    async (...args) => {
      running++;
      set({ busy: true, error: null });
      try {
        return await get()[method](...args);
      } catch (e) {
        set({ error: e });
        throw e;
      } finally {
        running--;
        if (running === 0) set({ busy: false });
      }
    };

  return {
    /** The Svelte store contract. */
    subscribe(run) {
      subscribers.add(run);
      run(state);
      return () => subscribers.delete(run);
    },
    generate: wrap('generate'),
    download: wrap('download'),
    open: wrap('open'),
    print: wrap('print'),
    preview: wrap('preview'),
    ping: wrap('ping'),
    clearError: () => set({ error: null }),
    close() {
      engine?.close();
      engine = null;
    },
  };
}
