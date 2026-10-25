import { createApp, h, onMounted, ref } from 'vue';
import { usePDFEngine } from '@lands-horizon-corp/pdf-engine-wasm/vue';
import { runSuite, publish, wasmUrl } from './suite.js';

createApp({
  setup() {
    const result = ref(null);
    const { generate, busy, error } = usePDFEngine({ wasm: wasmUrl });
    onMounted(async () => {
      const r = await runSuite();
      const bytes = await generate({ template: '<p>hook {{ n }}</p>', data: { n: 1 } });
      const failure = await generate({ template: '<p>\n{% if %}' }).then(() => null, (e) => e.code);
      r.hook = { header: String.fromCharCode(...bytes.subarray(0, 5)), failure, busyAfter: busy.value, errorCode: error.value?.code };
      result.value = r;
      publish(r);
    });
    return () => h('div', [h('h1', 'Vue'), h('pre', { id: 'result' }, result.value ? JSON.stringify(result.value, null, 2) : 'running…')]);
  },
}).mount('#app');
