import devServer, { defaultOptions } from '@hono/vite-dev-server';
import { defineConfig } from 'vite';

import { devServerRouteExclude } from './src/dev-server-routes';
import { installTerminalWebSocketBridge } from './src/terminal-ws-bridge';

export default defineConfig(({ command }) => ({
  plugins: command === 'serve'
    ? [{name:'hotsheet-terminal-websocket-bridge',configureServer:installTerminalWebSocketBridge},devServer({
        entry: 'src/dev-server.ts',
        exclude: [devServerRouteExclude, ...defaultOptions.exclude],
      })]
    : [],
  server: { host: '127.0.0.1', port: 4175, strictPort: true },
  build: { sourcemap: true },
}));
