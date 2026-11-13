import { type Context, Hono } from 'hono';

import type { Checkout } from './api';
import {
  CHANGE_MUX_PATH,
  type ChangeMuxForward,
  multiplexChangePolls,
  parseChangeMuxRequest,
} from './change-mux-bridge';
import type { ConversationExportPayload } from './conversation-export';
import { createConversationExportBridge } from './conversation-export-bridge';
import { createDevLatencyLog } from './dev-latency-log';
import { traceDevBridgeRequest } from './dev-request-timing';
import { isRecord } from './json-value';
import { REMOTE_MIGRATION_JOBS_PATH, REMOTE_PROJECT_OPEN_PATH } from './local-bridge-client';
import { type MigrationJobApi, migrationJobApi } from './migration-job-bridge';
import type { MigrationStart } from './migration-progress';
import {
  browserAuthRequest,
  chooseLocalFolder,
  closeRemoteServerSessions,
  connectGitTicketStoreRemote,
  createLocalGitTicketStore,
  gitTicketStoreConnectionId,
  listServerCheckouts,
  migrateHs1Project,
  openLocalProject,
  openRegisteredProject,
  openReviewedRemoteProject,
  proxyProjectRequest,
  recoverUnhealthyServer,
  remoteBrowserAuthRequired,
  remoteCsrfCookie,
  removeImportedHs1Data,
  revealCorruptTicket,
  type UnhealthyServerRecovery,
  unhealthyServerRecovery,
  UnregisteredRemoteProjectError,
} from './project-bridge';
import {
  beginRemotePairing,
  discoverRemoteProjects,
  finishRemotePairing,
  listRemoteServers,
  removeRemoteServer,
  selectedRemoteProjects,
  selectRemoteProject,
  unselectRemoteProject,
} from './remote-projects/host';

/** A remote session's polls ride the browser's own session cookie, as its project-api requests
 * do; the proxy adds the CSRF header from it and never forwards a loopback secret (HS2-ZVSMTZ). */
export function changePollSessionHeaders(browser: Headers): Headers {
  const headers = new Headers();
  const cookie = browser.get('cookie');
  if (cookie) headers.set('cookie', cookie);
  return headers;
}

/** Which grant a remote-mode bridge route needs (HS2-85W6BE, HS2-DXM0YN): any live session
 * (`session`), the admin grant (`management`), or the local grant (`host`). */
export function remoteRouteKind(path: string): 'session' | 'management' | 'host' {
  if (path.startsWith('/__hotsheet/remote-auth/owner')) return 'management';
  const remote =
    path === '/__hotsheet/checkouts' ||
    path === REMOTE_PROJECT_OPEN_PATH ||
    path === REMOTE_MIGRATION_JOBS_PATH ||
    path === CHANGE_MUX_PATH ||
    path.startsWith('/__hotsheet/project-api/') ||
    ['/__hotsheet/remote-auth/me', '/__hotsheet/remote-auth/rotate', '/__hotsheet/remote-auth/logout'].includes(path);
  return remote ? 'session' : 'host';
}

/** The grants the bridge reads from a `/remote-auth/me` answer; anything missing is not granted. */
export function remoteSessionGrants(device: unknown): { origin?: string; admin: boolean; local: boolean } {
  if (!isRecord(device)) return { admin: false, local: false };
  return {
    ...(typeof device.origin === 'string' ? { origin: device.origin } : {}),
    admin: device.admin === true,
    local: device.local === true,
  };
}

export function createDevApp(
  dev = true,
  reveal = revealCorruptTicket,
  chooseFolder: () => Promise<string | undefined> = () => chooseLocalFolder(),
  setupGit: (root: string, location?: string) => Promise<string> = createLocalGitTicketStore,
  connectRemote: (store: string, remote: string) => Promise<void> = connectGitTicketStoreRemote,
  migrate: (root: string, location?: string) => Promise<unknown> = migrateHs1Project,
  removeHs1: (project: string) => Promise<string[]> = removeImportedHs1Data,
  recover: (value: UnhealthyServerRecovery) => Promise<unknown> = recoverUnhealthyServer,
  listCheckouts: () => Promise<Checkout[]> = listServerCheckouts,
  jobs: MigrationJobApi = migrationJobApi,
  latencyLog: Pick<ReturnType<typeof createDevLatencyLog>, 'record' | 'recent' | 'file'> = createDevLatencyLog(),
  forwardChangePoll: (project: string, path: string, signal: AbortSignal, browser: Headers) => Promise<Response> = (
    project,
    path,
    signal,
    browser,
  ) =>
    proxyProjectRequest(
      project,
      path,
      new Request(new URL(path, 'http://bridge.invalid'), { headers: changePollSessionHeaders(browser) }),
      signal,
    ),
  openRemote: (root: string) => Promise<unknown> = openRegisteredProject,
  checkRemoteSession: (headers: Headers) => Promise<Response> = (headers) =>
    browserAuthRequest('/remote-auth/me', new Request('http://bridge.invalid/remote-auth/me', { headers })),
): Hono {
  const app = new Hono();
  app.use('/__hotsheet/*', async (context, next) => {
    if (!remoteBrowserAuthRequired()) return next();
    const path = context.req.path;
    if (
      path === '/__hotsheet/remote-auth/mode' ||
      path === '/__hotsheet/remote-auth/pair' ||
      path === '/__hotsheet/remote-auth/redeem'
    )
      return next();
    // HS2-85W6BE: trust comes from the session's grants, never from the hostname, peer address,
    // Host or forwarded headers. Tailscale Serve reaches this host through the same loopback
    // port the owner opens locally, so the request itself cannot tell the two apart.
    const route = remoteRouteKind(path);
    if (!context.req.header('cookie')) {
      if (route === 'host') return context.notFound();
      return context.json({ error: 'Remote session required.' }, 401);
    }
    const checked = await checkRemoteSession(context.req.raw.headers);
    if (!checked.ok) {
      if (route === 'host') return context.notFound();
      return context.json({ error: 'Remote session required.' }, 401);
    }
    const device = remoteSessionGrants(await checked.json().catch(() => undefined));
    // Device management needs the admin grant (HS2-DXM0YN); the server checks it again.
    if (route === 'management' && !device.admin) return context.json({ error: 'Administrator access required.' }, 403);
    // Host-only routes (folder chooser, open by path, setup) need the local grant.
    if (route === 'host' && !device.local) return context.notFound();
    if (context.req.method !== 'GET' && context.req.method !== 'HEAD') {
      // HS2-CD8YNK: compare with the origin the device paired from, as the server does. A
      // TLS-terminating proxy (Tailscale Serve) hands this host a plain-HTTP request, so the
      // request URL's origin is never the browser's https origin.
      const origin = context.req.header('origin');
      if (!origin || !device.origin || origin !== device.origin || !remoteCsrfCookie(context.req.raw.headers))
        return context.json({ error: 'Origin and CSRF token required.' }, 403);
    }
    return next();
  });
  app.get('/__hotsheet/remote-auth/mode', (context) => context.json({ required: remoteBrowserAuthRequired() }));
  app.all('/__hotsheet/remote-auth/*', async (context) => {
    const path = context.req.path.slice('/__hotsheet'.length);
    if (
      !/^\/remote-auth\/(?:pair|redeem|me|rotate|logout|owner(?:\/approve|\/reject|\/changes|\/revoke-all|\/devices\/[A-Za-z0-9]+)?)$/.test(
        path,
      )
    )
      return context.notFound();
    // The device-list long poll carries its cursor in the query (HS2-DXM0YN).
    const search = new URL(context.req.url).search;
    return browserAuthRequest(`${path}${search}`, context.req.raw, path.startsWith('/remote-auth/owner'));
  });
  // The development latency monitor's durable log (HS2-ZGBQ5Q): the browser posts each slow
  // interaction/request once (event-driven, never polled); GET reads the recent reports back.
  app.post('/__hotsheet/dev/latency', async (context) => {
    if (!dev) return context.notFound();
    try {
      return context.json({ accepted: await latencyLog.record(await context.req.json()) });
    } catch (error) {
      return context.json({ error: error instanceof Error ? error.message : 'Invalid latency report.' }, 400);
    }
  });
  app.get('/__hotsheet/dev/latency', (context) => {
    if (!dev) return context.notFound();
    return context.json({ file: latencyLog.file, reports: latencyLog.recent() });
  });
  const conversationExports = createConversationExportBridge(chooseFolder);
  // The cross-device project picker (a non-loopback client can't browse the filesystem) lists the
  // checkouts the bootstrap server knows about. Unlike the local-filesystem endpoints this is NOT
  // dev-gated — it is exactly the endpoint a remote client needs (HS2-QMR41J, HS2-VFNCXG).
  app.get('/__hotsheet/checkouts', async (context) => {
    try {
      return context.json(await listCheckouts());
    } catch (error) {
      return context.json(
        { error: error instanceof Error ? error.message : 'Could not list the projects open on the Hot Sheet server.' },
        502,
      );
    }
  });
  // Remote browsers open only checkouts the host already registered; the local route below can set up any
  // folder on this machine, so it stays off the remote allowlist (HS2-3HC9HT).
  app.post(REMOTE_PROJECT_OPEN_PATH, async (context) => {
    let root: unknown;
    try {
      root = (await context.req.json<{ root?: unknown }>()).root;
    } catch {
      return context.json({ error: 'Expected a JSON body with a project root.' }, 400);
    }
    if (typeof root !== 'string' || !root.trim()) return context.json({ error: 'Expected a project root.' }, 400);
    try {
      return context.json(await openRemote(root), 201);
    } catch (error) {
      if (error instanceof UnregisteredRemoteProjectError) return context.json({ error: error.message }, 403);
      return context.json({ error: error instanceof Error ? error.message : 'Could not open project.' }, 400);
    }
  });
  // A trusted desktop host owns the cross-server registry. In remote browser mode the auth
  // middleware above denies these routes; another device still sees only its one host's projects.
  app.get('/__hotsheet/remotes', async (context) => {
    if (remoteBrowserAuthRequired()) return context.notFound();
    try {
      return context.json(await listRemoteServers());
    } catch {
      return context.json({ error: 'Could not read approved remote servers.' }, 503);
    }
  });
  app.get('/__hotsheet/remotes/selected-projects', async (context) => {
    if (remoteBrowserAuthRequired()) return context.notFound();
    try {
      return context.json(await selectedRemoteProjects());
    } catch {
      return context.json({ error: 'Could not read remembered remote projects.' }, 503);
    }
  });
  app.post('/__hotsheet/remotes/pair', async (context) => {
    if (remoteBrowserAuthRequired()) return context.notFound();
    try {
      const { address, label, tls, reviewId } = await context.req.json<{
        address: string;
        label: string;
        tls?: { caFile?: string; certificateFile?: string; keyFile?: string };
        reviewId?: string;
      }>();
      return context.json(
        await beginRemotePairing(address, label, new URL(context.req.url).origin, tls, reviewId),
        201,
      );
    } catch (error) {
      return context.json({ error: error instanceof Error ? error.message : 'Could not start pairing.' }, 400);
    }
  });
  app.post('/__hotsheet/remotes/pair/:handle/redeem', async (context) => {
    if (remoteBrowserAuthRequired()) return context.notFound();
    try {
      const { discovery, revisedId } = await finishRemotePairing(context.req.param('handle'));
      if (revisedId) closeRemoteServerSessions(revisedId);
      return context.json(discovery);
    } catch (error) {
      return context.json({ error: error instanceof Error ? error.message : 'Could not finish pairing.' }, 409);
    }
  });
  app.get('/__hotsheet/remotes/:id/checkouts', async (context) => {
    if (remoteBrowserAuthRequired()) return context.notFound();
    try {
      return context.json(await discoverRemoteProjects(context.req.param('id')));
    } catch {
      return context.json({ error: 'Could not reach or authenticate the approved remote server.' }, 503);
    }
  });
  app.post('/__hotsheet/remotes/:id/projects/:checkout/open', async (context) => {
    if (remoteBrowserAuthRequired()) return context.notFound();
    try {
      const id = context.req.param('id');
      const checkout = context.req.param('checkout');
      const project = await openReviewedRemoteProject(id, checkout);
      await selectRemoteProject(id, checkout);
      return context.json(project, 201);
    } catch (error) {
      return context.json({ error: error instanceof Error ? error.message : 'Could not open remote project.' }, 409);
    }
  });
  app.delete('/__hotsheet/remotes/:id/projects/:checkout', async (context) => {
    if (remoteBrowserAuthRequired()) return context.notFound();
    try {
      return context.json(await unselectRemoteProject(context.req.param('id'), context.req.param('checkout')));
    } catch {
      return context.json({ error: 'Could not forget remote project.' }, 409);
    }
  });
  app.delete('/__hotsheet/remotes/:id', async (context) => {
    if (remoteBrowserAuthRequired()) return context.notFound();
    try {
      const revision = Number(context.req.query('revision'));
      if (!Number.isSafeInteger(revision) || revision <= 0) return context.json({ error: 'Revision required.' }, 400);
      const removed = await removeRemoteServer(context.req.param('id'), revision);
      closeRemoteServerSessions(removed.id);
      return context.json(removed);
    } catch {
      return context.json({ error: 'Could not remove approved remote server.' }, 409);
    }
  });
  app.post('/__hotsheet/projects/open', async (context) => {
    if (!dev) return context.notFound();
    try {
      const body = await context.req.json<{ root: string; ticketStore?: string }>();
      return context.json(await openLocalProject(body.root, body.ticketStore), 201);
    } catch (error) {
      return context.json(
        {
          error: error instanceof Error ? error.message : 'Could not open project.',
          recovery: unhealthyServerRecovery(error),
        },
        400,
      );
    }
  });
  app.post('/__hotsheet/server/recover-unhealthy', async (context) => {
    if (!dev) return context.notFound();
    try {
      await recover(await context.req.json<UnhealthyServerRecovery>());
      return context.json({ recovered: true });
    } catch (error) {
      return context.json(
        { error: error instanceof Error ? error.message : 'Could not recover the unresponsive server.' },
        409,
      );
    }
  });
  app.post('/__hotsheet/folders/choose', async (context) => {
    if (!dev) return context.notFound();
    try {
      return context.json({ path: await chooseFolder() });
    } catch (error) {
      return context.json(
        { error: error instanceof Error ? error.message : 'Could not open the folder chooser.' },
        400,
      );
    }
  });
  app.post('/__hotsheet/conversation-exports/destination', async (context) => {
    if (!dev) return context.notFound();
    try {
      const body = await context.req.json<{ suggestedName: string }>();
      return context.json({ destination: await conversationExports.chooseDestination(body.suggestedName) });
    } catch (error) {
      return context.json(
        { error: error instanceof Error ? error.message : 'Could not choose a conversation export destination.' },
        400,
      );
    }
  });
  app.post('/__hotsheet/conversation-exports/write', async (context) => {
    if (!dev) return context.notFound();
    try {
      return context.json(await conversationExports.write(await context.req.json<ConversationExportPayload>()), 201);
    } catch (error) {
      return context.json(
        { error: error instanceof Error ? error.message : 'Could not save the conversation export.' },
        400,
      );
    }
  });
  app.post('/__hotsheet/conversation-exports/open', async (context) => {
    if (!dev) return context.notFound();
    try {
      return context.json({ conversation: await conversationExports.open() });
    } catch (error) {
      return context.json(
        { error: error instanceof Error ? error.message : 'Could not open the saved conversation.' },
        400,
      );
    }
  });
  app.post('/__hotsheet/projects/setup-git', async (context) => {
    if (!dev) return context.notFound();
    try {
      const body = await context.req.json<{ root: string; location?: string }>(),
        ticketStore = await setupGit(body.root, body.location);
      return context.json({ ticketStore, connectionId: gitTicketStoreConnectionId(ticketStore) }, 201);
    } catch (error) {
      return context.json(
        { error: error instanceof Error ? error.message : 'Could not create the git ticket store.' },
        400,
      );
    }
  });
  app.post('/__hotsheet/projects/setup-git-remote', async (context) => {
    if (!dev) return context.notFound();
    try {
      const body = await context.req.json<{ store: string; remote: string }>();
      await connectRemote(body.store, body.remote);
      return context.json({ connected: true });
    } catch (error) {
      return context.json({ error: error instanceof Error ? error.message : 'Could not connect the Git remote.' }, 400);
    }
  });
  app.post('/__hotsheet/projects/migrate-hs1', async (context) => {
    if (!dev) return context.notFound();
    try {
      const body = await context.req.json<{ root: string; location?: string }>();
      return context.json(await migrate(body.root, body.location), 201);
    } catch (error) {
      return context.json(
        { error: error instanceof Error ? error.message : 'Could not import the Hot Sheet 1 project.' },
        400,
      );
    }
  });
  app.post('/__hotsheet/projects/migration-jobs', async (context) => {
    if (!dev) return context.notFound();
    try {
      return context.json(await jobs.start(await context.req.json<MigrationStart>()), 202);
    } catch (error) {
      return context.json({ error: error instanceof Error ? error.message : String(error) }, 409);
    }
  });
  const migrationStatus = async (context: Context, registeredOnly: boolean) => {
    if (!dev) return context.notFound();
    try {
      const root = context.req.query('root'),
        rawAfter = context.req.query('after');
      if (!root || (rawAfter !== undefined && (!Number.isSafeInteger(Number(rawAfter)) || Number(rawAfter) < 0)))
        throw new Error('A project root and valid revision are required.');
      // HS2-9FWETT: a remote device may read status only for a checkout the host registered.
      if (registeredOnly && !(await listCheckouts()).some((checkout) => checkout.root === root))
        return context.json({ error: new UnregisteredRemoteProjectError().message }, 403);
      return context.json({
        job: await jobs.watch(root, rawAfter === undefined ? undefined : Number(rawAfter), context.req.raw.signal),
      });
    } catch (error) {
      return context.json({ error: error instanceof Error ? error.message : String(error) }, 400);
    }
  };
  app.get('/__hotsheet/projects/migration-jobs', (context) => migrationStatus(context, false));
  app.get(REMOTE_MIGRATION_JOBS_PATH, (context) => migrationStatus(context, true));
  app.delete('/__hotsheet/projects/:project/hs1-data', async (context) => {
    if (!dev) return context.notFound();
    try {
      return context.json({ removed: await removeHs1(context.req.param('project')) });
    } catch (error) {
      return context.json(
        { error: error instanceof Error ? error.message : 'Could not remove the old Hot Sheet 1 data.' },
        400,
      );
    }
  });
  // Every open project's change long-poll over one browser request (HS2-6W1NNF), so projects
  // whose WebSockets fail cannot fill the browser's per-origin HTTP/1.1 connection pool.
  app.post(CHANGE_MUX_PATH, async (context) => {
    if (!dev) return context.notFound();
    let body: unknown;
    try {
      body = await context.req.json();
    } catch {
      return context.json({ error: 'Expected a JSON body.' }, 400);
    }
    const request = parseChangeMuxRequest(body);
    if (typeof request === 'string') return context.json({ error: request }, 400);
    const browser = context.req.raw.headers;
    const forward: ChangeMuxForward = (project, path, signal) => forwardChangePoll(project, path, signal, browser);
    return context.json(await multiplexChangePolls(request, forward, context.req.raw.signal));
  });
  app.all('/__hotsheet/project-api/:project/*', async (context) => {
    if (!dev) return context.notFound();
    const prefix = `/__hotsheet/project-api/${encodeURIComponent(context.req.param('project'))}`;
    const path = context.req.path.slice(prefix.length) || '/';
    return traceDevBridgeRequest((mark) =>
      proxyProjectRequest(
        context.req.param('project'),
        `${path}${new URL(context.req.url).search}`,
        context.req.raw,
        undefined,
        mark,
      ),
    );
  });
  app.post('/__hotsheet/projects/:project/corrupt-tickets/reveal', async (context) => {
    if (!dev) return context.notFound();
    try {
      const body = await context.req.json<{ path: string }>();
      await reveal(context.req.param('project'), body.path);
      return context.json({ revealed: true });
    } catch (error) {
      return context.json(
        { error: error instanceof Error ? error.message : 'Could not show the corrupt ticket file.' },
        400,
      );
    }
  });
  app.get('/ux-demo', (context) => {
    if (!dev) return context.notFound();
    return context.html(`<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <meta name="robots" content="noindex,nofollow" />
    <title>Hot Sheet UX components</title>
  </head>
  <body>
    <div id="ux-demo" class="kui-app-root"></div>
    <script type="module" src="/src/ux-demo/main.tsx"></script>
  </body>
</html>`);
  });
  return app;
}

// This entry is loaded only by Vite's `serve` command (see vite.config.ts). It is
// deliberately absent from the production build graph.
export default createDevApp(true);
