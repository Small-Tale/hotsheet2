import { defineConfig } from 'vite';

import remifyCss from './scripts/remify-css.mjs';

/**
 * The macOS screen saver's page (HS2-MZM988): one classic script and one stylesheet, because WebKit
 * refuses module scripts over `file://`, which is how the saver host loads its bundled page.
 */
export default defineConfig({
  publicDir: false,
  css: { postcss: { plugins: [remifyCss()] } },
  build: {
    outDir: 'dist-saver',
    emptyOutDir: true,
    cssCodeSplit: false,
    lib: {
      entry: 'src/standby-saver/main.tsx',
      name: 'HotSheetStandbySaver',
      formats: ['iife'],
      fileName: () => 'standby.js',
      cssFileName: 'standby',
    },
  },
});
