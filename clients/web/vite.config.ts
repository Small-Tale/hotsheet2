import { defineConfig } from 'vite';
import devServer, { defaultOptions } from '@hono/vite-dev-server';

export default defineConfig(({ command }) => ({
  plugins: command === 'serve'
    ? [devServer({
        entry: 'src/dev-server.ts',
        exclude: [/^(?!\/(?:ux-demo|__hotsheet\/dev-review)(?:[/?]|$)).*/, ...defaultOptions.exclude],
      })]
    : [],
  server: { host: '127.0.0.1', port: 4175, strictPort: true },
  build: { sourcemap: true },
}));
