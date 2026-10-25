/**
 * Vue 3 composable for pdf-engine in the browser.
 *
 *   <script setup>
 *   import { usePDFEngine } from '@lands-horizon-corp/pdf-engine-wasm/vue';
 *   const { download, busy, error } = usePDFEngine();
 *   </script>
 *   <template><button :disabled="busy" @click="download({ template, data }, 'invoice.pdf')">Download</button></template>
 *
 * `busy` and `error` are refs. One engine is made on first use and closed with the component. The methods still
 * reject, so you can also `try`/`catch`.
 */
import { getCurrentScope, onScopeDispose, ref } from 'vue';
import { PDFEngine } from './index.js';

export function usePDFEngine(options) {
  let engine = null;
  let running = 0;
  const busy = ref(false);
  const error = ref(null);

  const get = () => (engine ??= new PDFEngine(options));
  if (getCurrentScope()) {
    onScopeDispose(() => {
      engine?.close();
      engine = null;
    });
  }

  const wrap =
    (method) =>
    async (...args) => {
      running++;
      busy.value = true;
      error.value = null;
      try {
        return await get()[method](...args);
      } catch (e) {
        error.value = e;
        throw e;
      } finally {
        running--;
        if (running === 0) busy.value = false;
      }
    };

  return {
    generate: wrap('generate'),
    download: wrap('download'),
    open: wrap('open'),
    print: wrap('print'),
    preview: wrap('preview'),
    ping: wrap('ping'),
    busy,
    error,
    clearError: () => {
      error.value = null;
    },
  };
}
