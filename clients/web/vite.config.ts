import devServer, { defaultOptions } from '@hono/vite-dev-server';
import { defineConfig,type UserConfig } from 'vite';

import remifyCss from './scripts/remify-css.mjs';
import { devServerRouteExclude } from './src/dev-server-routes';
import { installTerminalWebSocketBridge } from './src/terminal-ws-bridge';

export function viteDependencyIsolation(environment:NodeJS.ProcessEnv=process.env):Pick<UserConfig,'cacheDir'|'optimizeDeps'> {
  const cacheDir=environment.HOTSHEET_VITE_CACHE_DIR;
  return {
    ...(cacheDir?{cacheDir}:{}),
    ...(environment.HOTSHEET_WEB_STABLE_DEV==='1'?{optimizeDeps:{noDiscovery:true,include:[]}}:{}),
  };
}

/**
 * HMR is off in the frozen stable-dev snapshot (`npm run dev`): workspace edits only appear after a
 * restart, so HMR gives nothing (HS2-8JV12R). The hot dev server (`npm run dev:hot`) and Playwright
 * keep HMR on.
 */
export function stableDevHmr(environment:NodeJS.ProcessEnv=process.env):false|undefined {
  return environment.HOTSHEET_WEB_STABLE_DEV==='1'?false:undefined;
}

/**
 * Whether `@hono/vite-dev-server` should inject its `<script>import("/@vite/client")</script>`.
 * `server.hmr:false` alone does not stop that client from connecting a websocket and full-reloading
 * the page on connection loss ("[vite] server connection lost. Polling for restart…") — the "web
 * client randomly restarts" bug, where a transient websocket drop under an event-loop stall triggers
 * a reload (HS2-8JV12R). In the frozen stable-dev snapshot we skip the client entirely: no client
 * means no websocket and no auto-reload at all, and the user reloads manually when they want fresh
 * code (which the snapshot needs a restart for anyway). Returns `false` in stable-dev, else the
 * default (undefined → the client is injected as usual for `npm run dev:hot` and Playwright).
 */
export function stableDevInjectClientScript(environment:NodeJS.ProcessEnv=process.env):false|undefined {
  return environment.HOTSHEET_WEB_STABLE_DEV==='1'?false:undefined;
}

export default defineConfig(({ command }) => ({
  ...viteDependencyIsolation(),
  plugins: command === 'serve'
    ? [{name:'hotsheet-terminal-websocket-bridge',configureServer:installTerminalWebSocketBridge},devServer({
        entry: 'src/dev-server.ts',
        exclude: [devServerRouteExclude, ...defaultOptions.exclude],
        injectClientScript: stableDevInjectClientScript(),
      })]
    : [],
  css: { postcss: { plugins: [remifyCss()] } },
  server: { host: '127.0.0.1', port: 4175, strictPort: true, hmr: stableDevHmr() },
  build: { sourcemap: true },
}));
