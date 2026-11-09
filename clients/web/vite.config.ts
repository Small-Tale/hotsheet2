import { createHash, randomBytes } from 'node:crypto';
import { fileURLToPath } from 'node:url';

import devServer, { defaultOptions } from '@hono/vite-dev-server';
import { uxReviewPlugin } from '@kerfjs/ui/vite-ux-review';
import { defineConfig, type HtmlTagDescriptor, type Plugin, type UserConfig } from 'vite';

import remifyCss from './scripts/remify-css.mjs';
import { scanBrowserDependencies } from './src/browser-dependency-scan';
import { CLIENT_BUILD_META_NAME } from './src/client-build';
import { localClientUrl, publishClientUrl } from './src/client-discovery';
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

/** Path prefix whose only successful answer is a browser-test server's own identity (HS2-BJ00D1). */
export const TEST_SERVER_IDENTITY_PATH = '/__hotsheet-test-server/';

/**
 * Playwright's readiness check only asks whether its URL answers. When two concurrent runs share a
 * port, the later run's Vite cannot bind, yet another run's server answers the shared URL, so the later
 * run tests the other worktree's source and loses its server when that run ends (HS2-BJ00D1). Each run
 * passes its own `HOTSHEET_WEB_TEST_SERVER_ID` and waits on this path, which answers 200 only for that
 * id: a foreign or id-less server returns 404, so the colliding run refuses instead of borrowing it.
 */
export function testServerIdentityPlugin(environment: NodeJS.ProcessEnv = process.env): Plugin {
  return {
    name: 'hotsheet-test-server-identity',
    configureServer(server) {
      const own = environment.HOTSHEET_WEB_TEST_SERVER_ID;
      server.middlewares.use((request, response, next) => {
        const path = request.url?.split('?', 1)[0] ?? '';
        if (!path.startsWith(TEST_SERVER_IDENTITY_PATH)) {
          next();
          return;
        }
        response.statusCode = own && path === `${TEST_SERVER_IDENTITY_PATH}${own}` ? 200 : 404;
        response.end();
      });
    },
  };
}

export function clientDiscoveryPlugin(): Plugin {
  return {
    name: 'hotsheet-client-discovery',
    configureServer(server) {
      const listener = server.httpServer;
      listener?.once('listening', () => {
        const address = listener.address();
        if (!address || typeof address === 'string') return;
        void publishClientUrl(localClientUrl(address.address, address.port))
          .then((dispose) => {
            if (!listener.listening) void dispose();
            else listener.once('close', () => void dispose());
          })
          .catch((error: unknown) => {
            server.config.logger.warn(`Could not publish Hot Sheet client discovery: ${String(error)}`);
          });
      });
    },
  };
}

/**
 * The production client build id (HS2-KPJWBK): a hash of the emitted script and stylesheet file
 * names. Vite content-hashes those names, so the same source yields the same id and any bundle change
 * yields a new one.
 */
export function bundleClientBuildId(fileNames: readonly string[]): string {
  const digest = createHash('sha256')
    .update([...fileNames].sort().join('\n'))
    .digest('hex');
  return `bundle-${digest.slice(0, 16)}`;
}

/** One id per development server process: a restarted dev server serves a new client (HS2-KPJWBK). */
export function developmentClientBuildId(now = Date.now(), random = randomBytes(4).toString('hex')): string {
  return `dev-${now.toString(36)}-${random}`;
}

function clientBuildMetaTag(id: string): HtmlTagDescriptor {
  return { tag: 'meta', attrs: { name: CLIENT_BUILD_META_NAME, content: id }, injectTo: 'head' };
}

/**
 * Stamp the client build id into `index.html` (HS2-KPJWBK). The page reads it to learn which bundle
 * it loaded; the host reads the same document to announce which bundle it serves.
 */
export function clientBuildIdPlugin(developmentId: string): Plugin {
  return {
    name: 'hotsheet-client-build-id',
    transformIndexHtml: {
      order: 'post',
      handler: (_html, context) => {
        if (!context.bundle) return [clientBuildMetaTag(developmentId)];
        const names = Object.values(context.bundle)
          .filter((output) => output.type === 'chunk' || output.fileName.endsWith('.css'))
          .map((output) => output.fileName);
        return [clientBuildMetaTag(bundleClientBuildId(names))];
      },
    },
  };
}

/** The browser mode belongs to the serving host, not to the reusable client bundle (HS2-7DBJMS). */
export function browserAuthModePlugin(): Plugin {
  return {
    name: 'hotsheet-browser-auth-mode',
    transformIndexHtml: () => [
      {
        tag: 'meta',
        attrs: {
          name: 'hotsheet-browser-auth',
          content: process.env.HOTSHEET_BROWSER_AUTH_MODE === 'remote' ? 'remote' : 'local',
        },
        injectTo: 'head',
      },
    ],
  };
}

export default defineConfig(({ command, isSsrBuild }) => {
  const developmentBuildId = developmentClientBuildId();
  return {
    ...viteDependencyIsolation(),
    plugins:
      command === 'serve'
        ? [
            // First, so no later middleware or SPA fallback can answer a foreign identity (HS2-BJ00D1).
            testServerIdentityPlugin(),
            clientDiscoveryPlugin(),
            // Serves `__ux-review/project.json` so the UX demo catalog can offer Kerf's Capture region link
            // (HS2-41YZSB); registered before the Hono dev server so its middleware answers first.
            uxReviewPlugin(),
            {
              name: 'hotsheet-project-websocket-bridge',
              configureServer: (server) => {
                const host = server.config.server.host;
                if (
                  (host === true || (typeof host === 'string' && !['127.0.0.1', '::1', 'localhost'].includes(host))) &&
                  process.env.HOTSHEET_BROWSER_AUTH_MODE !== 'remote'
                )
                  throw new Error('A non-loopback browser host requires HOTSHEET_BROWSER_AUTH_MODE=remote.');
                installProjectWebSocketBridge(server, undefined, undefined, undefined, () => developmentBuildId);
              },
            },
            devServer({
              entry: 'src/dev-server.ts',
              exclude: [devServerRouteExclude, ...defaultOptions.exclude],
              injectClientScript: stableDevInjectClientScript(),
            }),
            stableDevClientStripPlugin(),
            browserAuthModePlugin(),
            clientBuildIdPlugin(developmentBuildId),
          ]
        : [browserAuthModePlugin(), clientBuildIdPlugin(developmentBuildId)],
    define: { 'import.meta.env.HOTSHEET_WEB_STABLE_DEV': JSON.stringify(process.env.HOTSHEET_WEB_STABLE_DEV === '1') },
    css: { postcss: { plugins: [remifyCss()] } },
    server: { host: '127.0.0.1', port: 4175, strictPort: true, hmr: stableDevHmr() },
    build: { sourcemap: true },
    // The local production host bundle (`npm run build:host`) serves `dist/`; it needs no public assets.
    publicDir: isSsrBuild ? false : 'public',
  };
});
