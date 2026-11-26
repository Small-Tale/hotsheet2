import { execFile } from 'node:child_process';
import { appendFile, mkdir, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { promisify } from 'node:util';

import { expect, type Locator, type Page, test } from '@playwright/test';

import { withoutGitRepositoryEnv } from '../scripts/repository-env.mjs';
import { openMockedProject } from './open-mocked-project';
import { openUpstreamSocket, proxyFetch, type RealTicketFixtureContext, realTicketServer } from './real-ticket-server';
import { printThenEcho } from './terminal-fixture-command';

const execute = promisify(execFile);

// HS2-DHCY18: reproducible screenshots of the real app that illustrate the Structurizr architecture
// narrative (docs/architecture/narrative). Opt-in only: the normal gate never runs or rewrites them.
//   HS2_ARCHITECTURE_SCREENSHOTS=1 npx playwright test tests/architecture-screenshots.spec.ts --workers=1
// Every ticket, note, and name below is synthetic ("Acme Notes"); no store or customer content.
test.skip(!process.env.HS2_ARCHITECTURE_SCREENSHOTS, 'opt-in architecture screenshots (HS2-DHCY18)');
test.use({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1, colorScheme: 'light' });

const OUTPUT = fileURLToPath(new URL('../../../docs/architecture/narrative/screenshots/', import.meta.url));
const PROJECT = 'Acme Notes';
/** The folder the app shows for the project: portable, never the fixture's temporary directory. */
const SHOWN_ROOT = '/work/acme-notes';

type Server = Awaited<ReturnType<typeof realTicketServer>>;
interface Ticket {
  id: string;
  slug: string;
  connection_id: string;
  concurrency_token?: string;
}

/** A believable small product backlog: mixed categories, priorities, and statuses. */
const BACKLOG = [
  {
    title: 'Sync stalls when a note has more than 500 checklist items',
    category: 'bug',
    priority: 'highest',
    status: 'started',
    up_next: true,
    tags: ['sync'],
  },
  {
    title: 'Offline edits are lost after the app is force-quit',
    category: 'bug',
    priority: 'high',
    status: 'not_started',
    up_next: true,
    tags: ['offline'],
  },
  {
    title: 'Share a notebook with read-only collaborators',
    category: 'feature',
    priority: 'high',
    status: 'started',
    up_next: true,
    tags: ['sharing'],
  },
  {
    title: 'Markdown export keeps nested lists and code fences',
    category: 'feature',
    priority: 'default',
    status: 'not_started',
    tags: ['export'],
  },
  {
    title: 'Move attachment thumbnails to the background worker',
    category: 'task',
    priority: 'default',
    status: 'not_started',
    tags: ['performance'],
  },
  {
    title: 'Investigate slow cold start on large workspaces',
    category: 'investigation',
    priority: 'high',
    status: 'not_started',
    tags: ['performance'],
  },
  {
    title: 'Search ignores accented characters in note titles',
    category: 'issue',
    priority: 'default',
    status: 'not_started',
    tags: ['search'],
  },
  {
    title: 'Reminder notifications respect the quiet-hours setting',
    category: 'requirement_change',
    priority: 'low',
    status: 'not_started',
    tags: ['notifications'],
  },
  {
    title: 'Add keyboard shortcut for the command palette',
    category: 'feature',
    priority: 'low',
    status: 'completed',
    tags: ['keyboard'],
  },
  {
    title: 'Upgrade the rich-text editor dependency',
    category: 'task',
    priority: 'default',
    status: 'completed',
    tags: ['maintenance'],
  },
] as const;

const DETAILS = [
  'Opening a note with a very long checklist freezes the sync indicator at "Syncing…"; later edits never reach other devices.',
  '',
  '## Steps',
  '1. Create a note with 600 checklist items.',
  '2. Edit any item on device A.',
  '3. Watch device B: the edit never arrives.',
  '',
  '## Expected',
  'Large notes sync in chunks; the indicator returns to "Up to date".',
].join('\n');

/** The project folder is a Git checkout on `main` with one synthetic commit, as a real project is. */
async function acmeRepository({ root, env }: RealTicketFixtureContext) {
  const clean = withoutGitRepositoryEnv(env),
    git = (...args: string[]) => execute('git', args, { cwd: root, env: clean });
  await git('init', '-q', '-b', 'main');
  await writeFile(join(root, 'README.md'), '# Acme Notes\n\nA synthetic note-taking app.\n');
  await git('add', 'README.md');
  await git('commit', '-q', '-m', 'Start Acme Notes');
}

/** Seed the Acme Notes backlog through the real server API; returns the featured ticket. */
async function seedAcmeNotes(server: Server) {
  const created = new Map<(typeof BACKLOG)[number], Ticket>();
  // Newest first in the queue: create in reverse so the list reads top-down as written.
  for (const item of [...BACKLOG].reverse()) {
    const ticket = await server.request<Ticket>('/tickets', 'POST', {
      ...item,
      status: item.status === 'started' ? 'started' : 'not_started',
      tags: [...item.tags],
      details: item === BACKLOG[0] ? DETAILS : undefined,
    });
    if (item.status === 'completed')
      await server.request(`/tickets/${ticket.id}`, 'PATCH', {
        status: 'completed',
        actor: { role: 'human', id: 'dana' },
        no_commit: true,
      });
    created.set(item, ticket);
  }
  const featured = created.get(BACKLOG[0])!;
  const note = (text: string, actor: { role: string; id: string }, summary?: string) =>
    server.request(`/tickets/${featured.id}`, 'PATCH', {
      note: text,
      actor,
      ...(summary ? { note_summary: summary } : {}),
    });
  await note(
    'Reproduced with a synthetic 600-item checklist. The sync worker sends the whole note as one patch and the relay rejects bodies over 256 KB.',
    { role: 'human', id: 'dana' },
  );
  await note(
    [
      '## Preliminary thoughts',
      '- Root cause: one oversized patch per note.',
      '- Approach: split checklist patches into 64 KB chunks and resume from the last acknowledged chunk.',
      '- Risk tier: T1. Adds a regression test with a 600-item fixture.',
    ].join('\n'),
    { role: 'ai', id: 'claude-session' },
    'Planned chunked checklist sync',
  );
  return { featured, created };
}

/** Open the server's checkout in the real app through the dev bridge, named Acme Notes. */
async function openAcmeNotes(
  page: Page,
  server: Server,
  options: {
    /** Proxy terminal attach sockets to the server and show its real folder, which terminals launch under. */
    liveSockets?: boolean;
    routes?: (apiPath: string) => Promise<void>;
  } = {},
) {
  const root = options.liveSockets ? server.root : SHOWN_ROOT;
  const apiPath = `/__hotsheet/project-api/${server.checkoutId}`;
  await page.routeWebSocket(/.*/, (route) => {
    const url = new URL(route.url());
    const attach = /\/terminals\/([^/?]+)\/attach$/.exec(url.pathname);
    if (options.liveSockets && attach) {
      const target = new URL(`/terminals/${attach[1]}/attach`, server.url);
      target.protocol = 'ws:';
      for (const [key, value] of url.searchParams) target.searchParams.set(key, value);
      target.searchParams.set('secret', server.secret);
      const upstream = openUpstreamSocket(route, target);
      if (!upstream) return;
      const pending: (string | Buffer)[] = [];
      route.onMessage((message) => {
        if (upstream.readyState === upstream.OPEN) upstream.send(message);
        else pending.push(message);
      });
      upstream.on('open', () => {
        for (const message of pending.splice(0)) upstream.send(message);
      });
      upstream.on('message', (data: Buffer, binary) => {
        route.send(binary ? data : data.toString('utf8'));
      });
      return;
    }
    void route.close();
  });
  await page.route(/\/__hotsheet\/(?:remote\/)?projects\/open$/, (route) =>
    route.fulfill({
      status: 201,
      json: { id: server.checkoutId, root, name: PROJECT, stores: [`${root}.hs2`], apiPath },
    }),
  );
  await page.route(`**${apiPath}/**`, async (route) => {
    const incoming = new URL(route.request().url());
    try {
      const response = await proxyFetch(route, {
        url: `${server.url}${incoming.pathname.slice(apiPath.length)}${incoming.search}`,
        headers: { ...route.request().headers(), 'X-Hotsheet-Secret': server.secret },
        timeout: 60_000,
      });
      await route.fulfill({ response });
    } catch {
      await route.abort().catch(() => undefined);
    }
  });
  // Registered last, so these answer before the proxy above.
  await options.routes?.(apiPath);
  await page.goto('/');
  await openMockedProject(page);
}

/** Wait until every finite animation has finished, then two frames. */
async function settle(page: Page) {
  await page.evaluate(async () => {
    const finite = document
      .getAnimations()
      .filter((animation) => animation.effect?.getComputedTiming().iterations !== Infinity);
    await Promise.all(finite.map((animation) => animation.finished.catch(() => undefined)));
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
  });
}

async function shoot(page: Page, file: string, target?: Locator) {
  await mkdir(OUTPUT, { recursive: true });
  // No hover highlight from wherever the last click left the pointer.
  await page.mouse.move(1, 899);
  // The top-edge busy strip (HS2-MW1V3M) settles once the opening reads have answered.
  await expect(page.locator('[data-component="server-busy-bars"]')).toHaveAttribute('data-visible', 'false', {
    timeout: 20_000,
  });
  await settle(page);
  if (target) await target.screenshot({ path: `${OUTPUT}${file}`, animations: 'disabled' });
  else await page.screenshot({ path: `${OUTPUT}${file}`, animations: 'disabled' });
}

const row = (page: Page, slug: string) =>
  page.locator(`[data-action="select-ticket-row"][data-ticket-slug="${slug}"]`).first();

test('HS2-DHCY18 workspace and ticket editor', async ({ page }) => {
  test.setTimeout(180_000);
  const server = await realTicketServer({ seed: acmeRepository });
  try {
    const { featured } = await seedAcmeNotes(server);
    await openAcmeNotes(page, server);
    await expect(row(page, featured.slug)).toBeVisible();
    await shoot(page, '01-workspace-ticket-list.png');

    await row(page, featured.slug).click();
    const inspector = page.locator('#app-right-rail');
    await expect(inspector).toContainText('Root cause: one oversized patch per note.');
    await inspector.getByText('Root cause: one oversized patch per note.').scrollIntoViewIfNeeded();
    await page.evaluate(() => {
      (document.activeElement as HTMLElement | null)?.blur();
    });
    await shoot(page, '02-ticket-editor-notes.png');
  } finally {
    await server.stop(page);
  }
});

const CODEX_SESSION = '00000000-0000-7000-8000-0000000d4c18';
const CODEX_TERMINAL = 'codex-acme';
const TRANSCRIPT = [
  'codex  (synthetic session for Acme Notes)',
  '',
  '> Work HS2 ticket: sync stalls when a note has more than 500 checklist items',
  '',
  '  - Read src/sync/patch-queue.ts and src/sync/relay-client.ts',
  '  - Split checklist patches into 64 KB chunks',
  '  - Resume from the last acknowledged chunk after reconnect',
  '  - npm test -- sync      42 passed',
  '',
  '  Waiting for your answer before shipping.',
  '',
].join('\\r\\n');

test('HS2-DHCY18 AI session terminal and question prompt', async ({ page }) => {
  test.setTimeout(180_000);
  const server = await realTicketServer({ seed: acmeRepository });
  try {
    await seedAcmeNotes(server);
    const home = join(server.root, '.synthetic-codex-home');
    const day = join(home, 'sessions/2026/11/25');
    await mkdir(day, { recursive: true });
    const rollout = join(day, `rollout-2026-11-25T09-00-00-${CODEX_SESSION}.jsonl`);
    const line = (value: unknown) => `${JSON.stringify(value)}\n`;
    const now = () => new Date().toISOString();
    await writeFile(rollout, line({ timestamp: now(), type: 'session_meta', payload: { id: CODEX_SESSION } }));
    await server.request('/terminals', 'POST', {
      id: CODEX_TERMINAL,
      connect: 'codex',
      ...printThenEcho(TRANSCRIPT),
      cwd: server.root,
    });
    const report = await fetch(`${server.url}/terminals/${CODEX_TERMINAL}/ai-connection`, {
      method: 'POST',
      headers: { 'content-type': 'application/json', 'X-Hotsheet-Secret': server.secret },
      body: JSON.stringify({ agent: 'codex', session_id: CODEX_SESSION, codex_home: home }),
    });
    expect(report.status).toBe(204);
    await openAcmeNotes(page, server, { liveSockets: true });
    await page.getByRole('button', { name: 'Show Terminal Drawer' }).click();
    await page.getByRole('tab', { name: 'Codex Acme' }).click({ timeout: 20_000 });
    const session = page.locator(`[data-component="terminal-session"] [data-terminal-id="${CODEX_TERMINAL}"]`);
    await expect(session).toHaveAttribute('data-connection', 'connected');
    await expect(session.locator('.xterm-rows')).toContainText('Waiting for your answer');
    await shoot(page, '03-ai-session-terminal.png');

    await appendFile(
      rollout,
      line({
        timestamp: now(),
        type: 'event_msg',
        payload: {
          type: 'item_completed',
          item: {
            type: 'AgentMessage',
            id: 'item_synthetic_ship',
            delivery: 'async',
            questions: [
              {
                title: 'Ship chunked checklist sync behind the sync-v2 feature flag?',
                options: ['Yes, behind the flag', 'No, ship it to everyone'],
              },
            ],
          },
        },
      }),
    );
    const popup = page.getByRole('dialog', { name: 'AI waiting for your answer' });
    await expect(popup).toBeVisible({ timeout: 30_000 });
    await expect(popup.getByRole('radio', { name: 'Yes, behind the flag' })).toBeVisible();
    await shoot(page, '04-ai-question-prompt.png');
  } finally {
    await server.stop(page);
  }
});

const TARGET = '01ARZ3NDEKTSV4RRFFQ69G5FJ8';
const scheduler = { role: 'ai', id: 'scheduler-1' };

/** Acme Notes orchestrates itself (HS2-W27N1D): one goal, two planned steps, the first one running. */
async function orchestrateAcme(server: Server) {
  const base = `/checkouts/${server.checkoutId}/orchestration`;
  const added = await server.request<{ revision: number }>(`${base}/targets`, 'POST', {
    target_id: TARGET,
    label: 'Acme Notes',
    expected_revision: null,
  });
  const candidate = await server.request<{ destination: unknown }>(`${base}/inspect`, 'POST', {
    destination: { kind: 'local', checkout_id: server.checkoutId },
  });
  await server.request(`${base}/targets/${TARGET}/binding`, 'PUT', {
    target: { owner_checkout_id: server.checkoutId, target_id: TARGET },
    expected_manifest_revision: added.revision,
    expected_binding_revision: 0,
    previous_destination: null,
    destination: candidate.destination,
  });
  await server.request(`${base}/permission-rules`, 'POST', {
    target_id: TARGET,
    action: 'work_start',
    actor: { role: 'human', id: 'maintainer' },
  });
  await server.request(`${base}/project-type`, 'PUT', { project_type: 'orchestration', actor: { role: 'human' } });
  const create = (title: string, category: string) =>
    server.request<Ticket>('/tickets', 'POST', { title, category, priority: 'high' });
  const goal = await create('Ship read-only notebook sharing', 'feature');
  const first = await create('Add the share-link endpoint with read-only scopes', 'task');
  const second = await create('Add the Share dialog to the notebook toolbar', 'task');
  const define = (native: string, prerequisites: object[]) =>
    server.request(`${base}/readiness/define`, 'POST', {
      ticket: { native_id: goal.id },
      target_id: TARGET,
      connection_id: first.connection_id,
      native_id: native,
      prerequisites,
      actor: scheduler,
    });
  await define(first.id, []);
  await define(second.id, [
    { kind: 'ticket', target_id: TARGET, connection_id: first.connection_id, native_id: first.id, gates: 'none' },
  ]);
  await server.request(`${base}/runs`, 'POST', {
    ticket: { native_id: goal.id },
    target_id: TARGET,
    connection_id: first.connection_id,
    native_id: first.id,
    operation_id: 'op-acme-share',
    worker_id: 'worker-1',
    actor: scheduler,
  });
  return { goal, first, second };
}

test('HS2-DHCY18 orchestration plan and run', async ({ page }) => {
  test.setTimeout(180_000);
  const server = await realTicketServer({ seed: acmeRepository });
  try {
    const plan = await orchestrateAcme(server);
    await openAcmeNotes(page, server);
    await page.locator('[data-segment-value="orchestration"]').first().click();
    const view = page.locator('[data-orchestration-view="true"]');
    await expect(view).toBeVisible();
    await view.locator(`[data-ticket-slug="${plan.first.slug}"]`).first().click();
    const inspector = page.locator('[data-orchestration-inspector]');
    await expect(inspector.locator('[data-orchestration-section="Plan"]')).toContainText('In progress');
    await page.evaluate(() => {
      (document.activeElement as HTMLElement | null)?.blur();
    });
    await shoot(page, '05-orchestration-plan-run.png');
  } finally {
    await server.stop(page);
  }
});

test('HS2-DHCY18 remote device pairing request', async ({ page }) => {
  test.setTimeout(180_000);
  const server = await realTicketServer({ seed: acmeRepository });
  try {
    await seedAcmeNotes(server);
    const seconds = Math.floor(Date.now() / 1000);
    const state = { revision: 1, reads: 0, announced: false, pending: [] as unknown[] };
    const list = () => ({
      server_id: 'synthetic-server',
      revision: state.revision,
      pair_ttl_seconds: 600,
      manager: { local: true },
      devices: [],
      pending: state.pending,
    });
    let cursor = 1;
    await openAcmeNotes(page, server, {
      routes: async () => {
        await page.route('**/__hotsheet/remote-auth/owner**', (route) => {
          const url = new URL(route.request().url());
          if (url.pathname.endsWith('/changes') && Number(url.searchParams.get('after')) === state.revision)
            return new Promise<void>(() => undefined);
          state.reads += 1;
          return route.fulfill({ json: list() });
        });
        // The change channel: the server's anonymous `pairing_requests` event, once, after the request arrives.
        await page.route('**/ws/poll**', async (route) => {
          if (state.pending.length && !state.announced) {
            state.announced = true;
            cursor += 1;
            return route.fulfill({
              json: {
                cursor,
                events: [{ cursor, store: '', kind: 'pairing_requests', id: '', slug: '' }],
                overflow: false,
              },
            });
          }
          await new Promise((resolve) => setTimeout(resolve, 1_000));
          return route.fulfill({ json: { cursor, events: [], overflow: false } }).catch(() => undefined);
        });
      },
    });
    // The first read is the baseline; only a request that arrives after it is announced.
    await expect.poll(() => state.reads).toBeGreaterThanOrEqual(1);
    state.pending = [
      {
        id: '01SYNTHETICTABLET',
        label: 'Studio iPad',
        origin: 'https://studio-tablet.example',
        created_at: seconds - 5,
        expires_at: seconds + 600,
        approved: false,
      },
    ];
    state.revision += 1;
    const notice = page.locator('.app-toast', { hasText: 'asks to pair with this host' });
    await expect(notice).toBeVisible({ timeout: 20_000 });
    await page.locator('[data-segment-value="settings"]').click();
    await page.locator('[data-item-id="devices"]').click();
    await expect(page.locator('[data-component="remote-devices-settings"] [data-remote-pending]')).toHaveCount(1);
    await expect(notice).toBeVisible();
    await shoot(page, '06-remote-device-pairing.png');
  } finally {
    await server.stop(page);
  }
});

const MINUTE = 60_000;
/** One store's live snapshot in the server's `GET /live-state` shape; synthetic content only. */
function liveStore(id: string, name: string, checkout: string, word: 'active' | 'stalled', now: number) {
  const minute = Math.floor(now / MINUTE) * MINUTE;
  return {
    store: id,
    root: `/work/${id}.hs2`,
    name,
    checkouts: [checkout],
    epoch_ms: 1,
    revision: 1,
    state: word,
    activity: {
      word,
      active_since_ms: now - 42 * MINUTE,
      stopped_at_ms: null,
      last_progress_ms: now - (word === 'stalled' ? 12 : 2) * MINUTE,
      idle_deadline_ms: null,
      stalled: word === 'stalled' ? [{ slug: 'WEB-4K2P', worker: 'worker-2' }] : [],
      halted: [],
      causes: [],
    },
    claims: [],
    workers: { live: word === 'active' ? 2 : 0, expected: 1 },
    waiting: { count: 0, tickets: [] },
    rates: {
      tokens: {
        window_minutes: 5,
        minutes: [
          { start_ms: minute - 2 * MINUTE, tokens: 5_400 },
          { start_ms: minute - MINUTE, tokens: 7_100 },
          { start_ms: minute, tokens: 6_300 },
        ],
        peak_per_minute: 7_100,
      },
      tickets_updated: {
        window_minutes: 60,
        at_ms: [now - 4 * MINUTE, now - 11 * MINUTE, now - 25 * MINUTE, now - 38 * MINUTE],
        peak_per_hour: 20,
      },
    },
  };
}

test('HS2-DHCY18 standby display', async ({ page }) => {
  test.setTimeout(180_000);
  const server = await realTicketServer({ seed: acmeRepository });
  try {
    await seedAcmeNotes(server);
    const now = Date.now();
    await openAcmeNotes(page, server, {
      routes: async (apiPath) => {
        await page.route(`**${apiPath}/live-state**`, (route) =>
          route.fulfill({
            json: {
              epoch_ms: 1,
              stores: [
                liveStore('acme-notes', PROJECT, server.checkoutId, 'active', now),
                liveStore('acme-web', 'Acme Website', server.checkoutId, 'stalled', now),
              ],
            },
          }),
        );
      },
    });
    await page.getByRole('button', { name: 'Enter Standby' }).click();
    const screen = page.locator('[data-component="standby-screen"]');
    await expect(screen).toBeVisible();
    await expect(screen.locator('[data-role="standby-row"]')).toHaveCount(2);
    await shoot(page, '08-standby-display.png');
  } finally {
    await server.stop(page);
  }
});

test('HS2-DHCY18 settings ticket sources', async ({ page }) => {
  test.setTimeout(180_000);
  const server = await realTicketServer({ seed: acmeRepository });
  try {
    await seedAcmeNotes(server);
    await openAcmeNotes(page, server, {
      routes: async (apiPath) => {
        // The real provider list, with the store's temporary folder shown as a portable path.
        await page.route(`**${apiPath}/providers`, async (route) => {
          const response = await proxyFetch(route, {
            url: `${server.url}/providers`,
            headers: { ...route.request().headers(), 'X-Hotsheet-Secret': server.secret },
          });
          const providers = (await response.json()) as { locator?: string; display_name?: string }[];
          await route.fulfill({
            json: providers.map((provider) => ({
              ...provider,
              display_name: 'Acme Notes tickets',
              locator: `${SHOWN_ROOT}.hs2/tickets`,
            })),
          });
        });
      },
    });
    await page.locator('[data-segment-value="settings"]').click();
    await expect(page.getByText('Acme Notes tickets')).toBeVisible();
    await shoot(page, '07-settings-ticket-sources.png');
    await page.getByRole('button', { name: /^Data Source/ }).click();
    await shoot(page, '07b-settings-add-source.png');
  } finally {
    await server.stop(page);
  }
});
