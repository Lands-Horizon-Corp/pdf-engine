import { createApp, h, onMounted, ref } from 'vue';
import { runSuite, publish } from './suite.js';

createApp({
  setup() {
    const result = ref(null);
    onMounted(async () => {
      result.value = await runSuite();
      publish(result.value);
    });
    return () => h('div', [h('h1', 'Vue'), h('pre', { id: 'result' }, result.value ? JSON.stringify(result.value, null, 2) : 'running…')]);
  },
}).mount('#app');
