import { Hono } from 'hono';

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
import { type MigrationJobApi, migrationJobApi } from './migration-job-bridge';
import type { MigrationStart } from './migration-progress';
import {
  chooseLocalFolder,
  connectGitTicketStoreRemote,
  createLocalGitTicketStore,
  gitTicketStoreConnectionId,
  listServerCheckouts,
  migrateHs1Project,
  openLocalProject,
  proxyProjectRequest,
  recoverUnhealthyServer,
  removeImportedHs1Data,
  revealCorruptTicket,
  type UnhealthyServerRecovery,
  unhealthyServerRecovery,
} from './project-bridge';

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
  forwardChangePoll: ChangeMuxForward = (project, path, signal) =>
    proxyProjectRequest(project, path, new Request(new URL(path, 'http://bridge.invalid')), signal),
): Hono {
  const app = new Hono();
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
  app.get('/__hotsheet/projects/migration-jobs', async (context) => {
    if (!dev) return context.notFound();
    try {
      const root = context.req.query('root'),
        rawAfter = context.req.query('after');
      if (!root || (rawAfter !== undefined && (!Number.isSafeInteger(Number(rawAfter)) || Number(rawAfter) < 0)))
        throw new Error('A project root and valid revision are required.');
      return context.json({
        job: await jobs.watch(root, rawAfter === undefined ? undefined : Number(rawAfter), context.req.raw.signal),
      });
    } catch (error) {
      return context.json({ error: error instanceof Error ? error.message : String(error) }, 400);
    }
  });
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
    return context.json(await multiplexChangePolls(request, forwardChangePoll, context.req.raw.signal));
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
