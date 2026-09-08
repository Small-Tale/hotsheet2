import devServer, { defaultOptions } from '@hono/vite-dev-server';
import { defineConfig,type UserConfig } from 'vite';

import { devServerRouteExclude } from './src/dev-server-routes';
import { installTerminalWebSocketBridge } from './src/terminal-ws-bridge';

export function viteDependencyIsolation(environment:NodeJS.ProcessEnv=process.env):Pick<UserConfig,'cacheDir'|'optimizeDeps'> {
  const cacheDir=environment.HOTSHEET_VITE_CACHE_DIR;
  return {
    ...(cacheDir?{cacheDir}:{}),
    ...(environment.HOTSHEET_WEB_STABLE_DEV==='1'?{optimizeDeps:{noDiscovery:true,include:[]}}:{}),
  };
}

export default defineConfig(({ command }) => ({
  ...viteDependencyIsolation(),
  plugins: command === 'serve'
    ? [{name:'hotsheet-terminal-websocket-bridge',configureServer:installTerminalWebSocketBridge},devServer({
        entry: 'src/dev-server.ts',
        exclude: [devServerRouteExclude, ...defaultOptions.exclude],
      })]
    : [],
  server: { host: '127.0.0.1', port: 4175, strictPort: true },
  build: { sourcemap: true },
}));
