import { resolve } from 'node:path';
import { defineConfig, externalizeDepsPlugin } from 'electron-vite';
import react from '@vitejs/plugin-react';

const shared = { '@nostalgia/shared': resolve(__dirname, '../shared/src/index.ts') };

export default defineConfig({
  main: {
    plugins: [externalizeDepsPlugin({ exclude: ['@nostalgia/shared'] })],
    resolve: { alias: shared },
  },
  preload: {
    plugins: [externalizeDepsPlugin()],
    build: { rollupOptions: { output: { format: 'cjs', entryFileNames: '[name].cjs' } } },
  },
  renderer: {
    resolve: { alias: shared },
    plugins: [react()],
  },
});
