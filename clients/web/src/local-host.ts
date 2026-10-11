/**
 * Local production host (HS2-587N4D): serves the built web client (`dist/`) with the same local
 * bridge the Vite dev server mounts, without loading Vite. `npm run prod` builds both and starts it.
 *
 * The bridge is `createDevApp(true)`: every local endpoint (project open, API proxy with the server
 * secret, folder picking, migration) stays enabled because this is the same local-machine host as
 * `npm run dev`. The development-only surfaces, Dev Review, the `/ux-demo` catalog, and its
 * modification feed, are not served; the production bundle already omits their client code.
 */
import { chmodSync, copyFileSync, cpSync, existsSync, mkdirSync, mkdtempSync, rmSync, statSync } from 'node:fs';
import { createServer, type Server } from 'node:http';
import { tmpdir } from 'node:os';
import { isAbsolute, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { getRequestListener } from '@hono/node-server';
import { serveStatic } from '@hono/node-server/serve-static';
import { Hono, type MiddlewareHandler } from 'hono';

import { spawnSync } from './child-process';
import { localClientUrl, publishClientUrl } from './client-discovery';
import { createDevApp } from './dev-server';
import { isRecord, parseJson } from './json-value';
import { developmentRepositoryRoot } from './project-bridge';
import { installProjectWebSocketBridge } from './terminal-ws-bridge';

export const LOCAL_HOST_DEFAULT_PORT = 4175;

interface BinaryRevisionStatus {
  build_revision?: string | null;
  source_revision?: string | null;
  source_stale?: boolean;
}

const nullableString = (value: unknown) => value === undefined || value === null || typeof value === 'string';

/** Validate `--revision-status` output; any other shape is treated as unknown (HS2-3DA0FQ). */
export function parseBinaryRevisionStatus(value: unknown): BinaryRevisionStatus | undefined {
  if (
    !isRecord(value) ||
    !nullableString(value.build_revision) ||
    !nullableString(value.source_revision) ||
    (value.source_stale !== undefined && typeof value.source_stale !== 'boolean')
  )
    return undefined;
  return value;
}

function binaryRevisionStatus(path: string): BinaryRevisionStatus | undefined {
  try {
    const result = spawnSync(path, ['--revision-status'], {
      encoding: 'utf8',
      timeout: 10_000,
      stdio: ['ignore', 'pipe', 'ignore'],
    });
    return result.status === 0 ? parseBinaryRevisionStatus(parseJson(result.stdout.toString())) : undefined;
  } catch {
    return undefined;
  }
}

function currentRevision(status: BinaryRevisionStatus | undefined): string | undefined {
  if (!status || status.source_stale !== false || !status.build_revision) return undefined;
  return status.build_revision === status.source_revision ? status.build_revision : undefined;
}

/** The Rust binaries the bridge launches, by the environment variable that overrides each. */
const HOST_BINARIES = {
  HOTSHEET_SERVER_BIN: 'hotsheet-server',
  HOTSHEET_CLI_BIN: 'hotsheet-cli',
  HOTSHEET_MIGRATE_BIN: 'hotsheet-migrate',
} as const;

/**
 * Release builds of the bridge's binaries for a production host (HS2-D2JQ9A). The bridge defaults to
 * `target/debug`, whose server is many times slower at hashing and walking stores. When a release
 * server is built, each binary prefers its current release build and falls back to its current
 * debug build independently. A local binary is eligible only when its embedded revision matches
 * the current workspace source and the selected server revision.
 */
export function releaseBinaryEnvironment(
  repositoryRoot: string,
  environment: Readonly<Record<string, string | undefined>> = process.env,
  exists: (path: string) => boolean = existsSync,
  inspect: (path: string) => BinaryRevisionStatus | undefined = binaryRevisionStatus,
): Record<string, string> {
  const release = (name: string) => resolve(repositoryRoot, 'target/release', name);
  const debug = (name: string) => resolve(repositoryRoot, 'target/debug', name);
  const chosen: Record<string, string> = {};
  let expectedRevision: string | undefined;
  for (const [variable, name] of Object.entries(HOST_BINARIES)) {
    if (environment[variable]) continue;
    let selected: { path: string; revision: string } | undefined;
    for (const path of [release(name), debug(name)]) {
      const revision = exists(path) ? currentRevision(inspect(path)) : undefined;
      if (revision && (!expectedRevision || revision === expectedRevision)) {
        selected = { path, revision };
        break;
      }
    }
    if (!selected)
      throw new Error(
        `No current Hot Sheet ${name} binary is available. Run \`npm run server:rebuild\` or \`npm run server:rebuild:release\`.`,
      );
    chosen[variable] = selected.path;
    if (variable === 'HOTSHEET_SERVER_BIN') {
      expectedRevision = selected.revision;
      chosen.HOT_SHEET_BUILD_REVISION = selected.revision;
    } else if (!expectedRevision) {
      expectedRevision = selected.revision;
    }
  }
  return chosen;
}

/** Keep a production host on the artifacts it selected at launch while working builds replace dist/ and target/. */
export function snapshotLocalHostArtifacts(
  distRoot: string,
  binaryEnvironment: Record<string, string>,
): { distRoot: string; environment: Record<string, string>; dispose: () => void } {
  const snapshotRoot = mkdtempSync(resolve(tmpdir(), 'hotsheet-local-host-')),
    environment = { ...binaryEnvironment },
    dispose = () => {
      rmSync(snapshotRoot, { recursive: true, force: true });
    };
  try {
    const pinnedDistRoot = resolve(snapshotRoot, 'dist');
    cpSync(distRoot, pinnedDistRoot, { recursive: true, dereference: true });
    mkdirSync(resolve(snapshotRoot, 'bin'));
    for (const [variable, name] of Object.entries(HOST_BINARIES)) {
      const source = environment[variable];
      if (!source || !isAbsolute(source)) continue;
      const pinned = resolve(snapshotRoot, 'bin', name);
      copyFileSync(source, pinned);
      chmodSync(pinned, statSync(source).mode & 0o777);
      environment[variable] = pinned;
    }
    return { distRoot: pinnedDistRoot, environment, dispose };
  } catch (error) {
    dispose();
    throw error;
  }
}

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
      const address = server.address();
      if (!address || typeof address === 'string') {
        reject(new Error('Hot Sheet client listener has no network address.'));
        return;
      }
      void publishClientUrl(localClientUrl(address.address, address.port))
        .then((dispose) => {
          server.once('close', () => void dispose());
          resolveListen(server);
        })
        .catch((error: unknown) => {
          server.close();
          reject(error instanceof Error ? error : new Error(String(error)));
        });
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
  const release = releaseBinaryEnvironment(developmentRepositoryRoot());
  const selected = { ...release };
  for (const variable of Object.keys(HOST_BINARIES)) {
    if (process.env[variable]) selected[variable] = process.env[variable];
  }
  const snapshot = snapshotLocalHostArtifacts(fileURLToPath(new URL('../dist', import.meta.url)), selected);
  Object.assign(process.env, snapshot.environment);
  console.log(
    'HOTSHEET_SERVER_BIN' in release && release.HOTSHEET_SERVER_BIN.includes('/target/release/')
      ? `Using release Hot Sheet binaries (${release.HOTSHEET_SERVER_BIN}).`
      : 'Using debug Hot Sheet binaries; run `npm run server:rebuild:release` for a faster server.',
  );
  let server: Server;
  try {
    server = await startLocalHost({ host, port, distRoot: snapshot.distRoot });
  } catch (error) {
    snapshot.dispose();
    throw error;
  }
  server.once('close', snapshot.dispose);
  const address = server.address(),
    listening = typeof address === 'object' && address ? address.port : port;
  console.log(`Hot Sheet production client on http://${host}:${listening}/`);
  const stop = () => {
    server.close(() => {
      snapshot.dispose();
      process.exit(0);
    });
    server.closeAllConnections();
  };
  process.once('SIGINT', stop);
  process.once('SIGTERM', stop);
}
