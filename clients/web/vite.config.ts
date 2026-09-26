import { fileURLToPath } from 'node:url';

import devServer, { defaultOptions } from '@hono/vite-dev-server';
import { defineConfig, type Plugin, type UserConfig } from 'vite';

import remifyCss from './scripts/remify-css.mjs';
import { scanBrowserDependencies } from './src/browser-dependency-scan';
import { devServerRouteExclude } from './src/dev-server-routes';
import { installProjectWebSocketBridge } from './src/terminal-ws-bridge';

/**
 * The frozen stable-dev snapshot never discovers dependencies at runtime (a mid-session re-optimize
 * reloads open pages, HS2-ATE664), but it pre-bundles every dependency the browser import graph
 * reaches once at startup, so a new tab loads a few bundles instead of ~2,400 raw `node_modules`
 * files (HS2-N9RD7X).
 */
export function viteDependencyIsolation(
  environment: NodeJS.ProcessEnv = process.env,
  browserDependencies: () => string[] = () => scanBrowserDependencies(fileURLToPath(new URL('.', import.meta.url))),
): Pick<UserConfig, 'cacheDir' | 'optimizeDeps'> {
  const cacheDir = environment.HOTSHEET_VITE_CACHE_DIR;
  return {
    ...(cacheDir ? { cacheDir } : {}),
    ...(environment.HOTSHEET_WEB_STABLE_DEV === '1'
      ? { optimizeDeps: { noDiscovery: true, include: browserDependencies() } }
      : {}),
  };
}

/**
 * HMR is off in the frozen stable-dev snapshot (`npm run dev`): workspace edits only appear after a
 * restart, so HMR gives nothing (HS2-8JV12R). The hot dev server (`npm run dev:hot`) and Playwright
 * keep HMR on.
 */
export function stableDevHmr(environment: NodeJS.ProcessEnv = process.env): false | undefined {
  return environment.HOTSHEET_WEB_STABLE_DEV === '1' ? false : undefined;
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
export function stableDevInjectClientScript(environment: NodeJS.ProcessEnv = process.env): false | undefined {
  return environment.HOTSHEET_WEB_STABLE_DEV === '1' ? false : undefined;
}

/** Remove Vite core's external dev-client tag from index HTML in frozen stable-dev. Hono's
 * `injectClientScript` option controls only Hono-rendered responses; Vite's own index middleware
 * independently injects this tag for `/`, including when `server.hmr` is false (HS2-8JV12R). */
export function stripStableDevClientTag(html: string, environment: NodeJS.ProcessEnv = process.env): string {
  if (environment.HOTSHEET_WEB_STABLE_DEV !== '1') return html;
  return html.replace(
    /<script\b(?=[^>]*\btype=["']module["'])(?=[^>]*\bsrc=["'][^"']*\/@vite\/client["'])[^>]*><\/script>\s*/giu,
    '',
  );
}

export function stableDevClientStripPlugin(environment: NodeJS.ProcessEnv = process.env): Plugin {
  return {
    name: 'hotsheet-stable-dev-client-strip',
    configureServer(server) {
      if (environment.HOTSHEET_WEB_STABLE_DEV !== '1') return;
      server.middlewares.use((request, response, next) => {
        if (request.url?.split('?', 1)[0] !== '/@vite/client') {
          next();
          return;
        }
        response.statusCode = 200;
        response.setHeader('content-type', 'text/javascript');
        response.end('export * from "/src/stable-vite-client.ts";\n');
      });
    },
    transformIndexHtml: { order: 'post', handler: (html) => stripStableDevClientTag(html, environment) },
  };
}

export default defineConfig(({ command }) => ({
  ...viteDependencyIsolation(),
  plugins:
    command === 'serve'
      ? [
          { name: 'hotsheet-project-websocket-bridge', configureServer: installProjectWebSocketBridge },
          devServer({
            entry: 'src/dev-server.ts',
            exclude: [devServerRouteExclude, ...defaultOptions.exclude],
            injectClientScript: stableDevInjectClientScript(),
          }),
          stableDevClientStripPlugin(),
        ]
      : [],
  define: { 'import.meta.env.HOTSHEET_WEB_STABLE_DEV': JSON.stringify(process.env.HOTSHEET_WEB_STABLE_DEV === '1') },
  css: { postcss: { plugins: [remifyCss()] } },
  server: { host: '127.0.0.1', port: 4175, strictPort: true, hmr: stableDevHmr() },
  build: { sourcemap: true },
}));
