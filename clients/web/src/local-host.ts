/**
 * Local production host (HS2-587N4D): serves the built web client (`dist/`) with the same local
 * bridge the Vite dev server mounts, without loading Vite. `npm run prod` builds both and starts it.
 *
 * The bridge is `createDevApp(true)`: every local endpoint (project open, API proxy with the server
 * secret, folder picking, migration) stays enabled because this is the same local-machine host as
 * `npm run dev`. The development-only surfaces, Dev Review, the `/ux-demo` catalog, and its
 * modification feed, are not served; the production bundle already omits their client code.
 */
import { createServer, type Server } from 'node:http';
import { fileURLToPath } from 'node:url';

import { getRequestListener } from '@hono/node-server';
import { serveStatic } from '@hono/node-server/serve-static';
import { Hono, type MiddlewareHandler } from 'hono';

import { createDevApp } from './dev-server';
import { installProjectWebSocketBridge } from './terminal-ws-bridge';

export const LOCAL_HOST_DEFAULT_PORT = 4175;

/** Set `Cache-Control` on a successful response produced by the handlers after this middleware. */
function cacheControl(value: string): MiddlewareHandler {
  return async (context, next) => {
    await next();
    if (context.res.status !== 200) return;
    const headers = new Headers(context.res.headers);
    headers.set('Cache-Control', value);
    context.res = new Response(context.res.body, { status: context.res.status, headers });
  };
}
const DEVELOPMENT_ONLY = /^\/(?:ux-demo|__hotsheet\/(?:dev-review|demo-modified))(?:[/?]|$)/;

/** Build the host app: development-only routes 404, the bridge owns `/__hotsheet`, the rest is static. */
export function createLocalHostApp(distRoot: string, bridge: Hono = createDevApp(true)): Hono {
  const app = new Hono();
  app.use('*', async (context, next) => {
    if (DEVELOPMENT_ONLY.test(context.req.path)) return context.notFound();
    await next();
  });
  app.route('/', bridge);
  // Hashed build assets never change for a given URL; the application document always revalidates.
  app.use('/assets/*', cacheControl('public, max-age=31536000, immutable'));
  app.use('/assets/*', serveStatic({ root: distRoot }));
  app.all('/assets/*', (context) => context.notFound());
  app.use('*', serveStatic({ root: distRoot }));
  // Client-side routes resolve to the application document.
  app.get('*', cacheControl('no-cache'), serveStatic({ root: distRoot, path: 'index.html' }));
  return app;
}

export function startLocalHost({
  host = '127.0.0.1',
  port = LOCAL_HOST_DEFAULT_PORT,
  distRoot = fileURLToPath(new URL('../dist', import.meta.url)),
}: { host?: string; port?: number; distRoot?: string } = {}): Promise<Server> {
  const listener = getRequestListener(createLocalHostApp(distRoot).fetch),
    server = createServer((request, response) => {
      void listener(request, response);
    });
  installProjectWebSocketBridge({ httpServer: server });
  return new Promise((resolveListen, reject) => {
    server.once('error', reject);
    server.listen(port, host, () => {
      resolveListen(server);
    });
  });
}

function argument(name: string): string | undefined {
  const index = process.argv.indexOf(`--${name}`);
  return index >= 0 ? process.argv[index + 1] : undefined;
}

// Started directly (`node dist-host/local-host.js [--host <address>] [--port <port>]`).
if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const host = argument('host') ?? '127.0.0.1',
    port = Number(argument('port') ?? LOCAL_HOST_DEFAULT_PORT);
  const server = await startLocalHost({ host, port });
  const address = server.address(),
    listening = typeof address === 'object' && address ? address.port : port;
  console.log(`Hot Sheet production client on http://${host}:${listening}/`);
  const stop = () => {
    server.close(() => process.exit(0));
    server.closeAllConnections();
  };
  process.once('SIGINT', stop);
  process.once('SIGTERM', stop);
}
