import react from '@vitejs/plugin-react';
import { resolve } from 'node:path';
import { defineConfig } from 'vite';

export default defineConfig({
  plugins: [react()],
  build: {
    rollupOptions: {
      input: {
        plain: resolve(import.meta.dirname, 'plain/index.html'),
        react: resolve(import.meta.dirname, 'react/index.html'),
        vue: resolve(import.meta.dirname, 'vue/index.html'),
      },
    },
  },
});
