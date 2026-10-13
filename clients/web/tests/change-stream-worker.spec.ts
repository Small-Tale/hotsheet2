import { expect, type Page, test, type WebSocketRoute, type Worker } from '@playwright/test';

/**
 * HS2-SH9179: the shared change-stream worker, opted in through its local setting, follows two
 * projects' live streams from a dedicated worker while the user switches, searches, and updates.
 */
const projects = {
  alpha: {
    id: 'alpha-project',
    root: '/work/alpha',
    name: 'Alpha',
    stores: ['/work/alpha.hs2'],
    apiPath: '/__hotsheet/project-api/alpha-project',
  },
  beta: {
    id: 'beta-project',
    root: '/work/beta',
    name: 'Beta',
    stores: ['/work/beta.hs2'],
    apiPath: '/__hotsheet/project-api/beta-project',
  },
} as const;
const row = (id: string, title: string, upNext = true) => ({
  connection_id: 'git',
  category: 'bug',
  priority: 'default',
  status: 'not_started',
  feedback_needed: false,
  tags: [],
  blocked_by: [],
  claim_count: 0,
  created_at: '2026-09-11T00:00:00Z',
  updated_at: '2026-09-11T00:01:00Z',
  native_id: id,
  qualified_id: `git:${id}`,
  id,
  slug: `HS2-${id}`,
  title,
  up_next: upNext,
});
type FixtureTicket = ReturnType<typeof row>;
const ticketPage = (tickets: FixtureTicket[]) => ({
  items: tickets,
  counts: {
    total: tickets.length,
    queued: tickets.length,
    backlog: 0,
    archive: 0,
    trash: 0,
    open: tickets.length,
    up_next: tickets.filter((ticket) => ticket.up_next).length,
    active: 0,
    started: 0,
    completed_today: 0,
    completion_trend: [],
  },
});
const capabilities = {
  create: true,
  update: true,
  close: true,
  notes: true,
  note_edit: true,
  note_delete: true,
  attachments: true,
  assignment: true,
  review_requests: true,
  dependencies: true,
  up_next: true,
  close_reasons: true,
  claims: true,
  atomic_batch: true,
  not_working_report: true,
  offline_mutation: true,
  history: true,
  watch: true,
  provider_idempotency: true,
  query_fields: [],
};

async function installProjects(page: Page) {
  const rows: Record<string, FixtureTicket[]> = {
    'alpha-project': [row('ALPHA1', 'Alpha first')],
    'beta-project': [row('BETA01', 'Beta first')],
  };
  const sockets = new Map<string, WebSocketRoute[]>(),
    cursor = new Map<string, number>(),
    polls: string[] = [],
    log = new Map<string, Array<{ cursor: number; [key: string]: unknown }>>(),
    held = new Map<string, Array<{ since: number; finish: () => void }>>();
  const eventsAfter = (projectId: string, since: number) =>
    (log.get(projectId) ?? []).filter((item) => item.cursor > since);
  const ticketLoads = new Map<string, number>();
  await page.routeWebSocket(/\/__hotsheet\/project-api\/[^/]+\/ws\/sync(?:\?.*)?$/, (socket) => {
    const projectId = decodeURIComponent(new URL(socket.url()).pathname.match(/project-api\/([^/]+)/)?.[1] ?? '');
    sockets.set(projectId, [...(sockets.get(projectId) ?? []), socket]);
    socket.onClose(() => {
      sockets.set(
        projectId,
        (sockets.get(projectId) ?? []).filter((candidate) => candidate !== socket),
      );
    });
  });
  await page.route('**/*', async (route) => {
    const request = route.request(),
      url = new URL(request.url()),
      path = decodeURIComponent(url.pathname);
    if (path === '/__hotsheet/projects/open') {
      const root = (request.postDataJSON() as { root: string }).root;
      return route.fulfill({ status: 201, json: root === projects.beta.root ? projects.beta : projects.alpha });
    }
    if (path === '/__hotsheet/folders/choose') return route.fulfill({ json: { path: projects.beta.root } });
    const projectId = path.match(/project-api\/([^/]+)/)?.[1],
      tickets = projectId ? (rows[projectId] ?? []) : [];
    if (path.endsWith('/ws/poll') && projectId) {
      polls.push(`${projectId}?${url.searchParams.toString()}`);
      cursor.set(projectId, cursor.get(projectId) ?? 1);
      const sinceParam = url.searchParams.get('since'),
        timeout = Math.min(Number(url.searchParams.get('timeout_ms') ?? 0), 2_000);
      const respond = () =>
        route
          .fulfill({
            json: {
              cursor: cursor.get(projectId),
              events: sinceParam === null ? [] : eventsAfter(projectId, Number(sinceParam)),
              overflow: false,
            },
          })
          .catch(() => undefined);
      // A genuine long poll: an idle request stays pending until an event or its bounded timeout.
      if (sinceParam === null || timeout === 0 || eventsAfter(projectId, Number(sinceParam)).length) return respond();
      await new Promise<void>((resolve) => {
        let done = false;
        const finish = () => {
          if (done) return;
          done = true;
          held.set(
            projectId,
            (held.get(projectId) ?? []).filter((item) => item.finish !== finish),
          );
          resolve();
        };
        held.set(projectId, [...(held.get(projectId) ?? []), { since: Number(sinceParam), finish }]);
        setTimeout(finish, timeout);
      });
      return respond();
    }
    if (path.endsWith('/providers'))
      return route.fulfill({
        json: [
          {
            connection_id: 'git',
            provider: 'git',
            display_name: 'Hot Sheet git',
            locator: `/work/${projectId}.hs2`,
            default: true,
            capabilities,
          },
        ],
      });
    if (
      ['/provider-connections', '/corrupt-tickets', '/commands', '/command-runs', '/views', '/terminals'].some(
        (suffix) => path.endsWith(suffix),
      ) ||
      ['/permissions', '/connections', '/drive/sessions'].some((suffix) => path.endsWith(suffix))
    )
      return route.fulfill({ json: [] });
    if (path.endsWith('/repository/status'))
      return route.fulfill({
        json: { branch: 'main', ahead: 0, behind: 0, staged: 0, unstaged: 0, untracked: 0, conflicted: 0, clean: true },
      });
    if (path.endsWith('/terminal-settings')) return route.fulfill({ json: { inherit_global_shell_history: false } });
    if (path.endsWith('/tickets') && request.method() === 'GET') {
      ticketLoads.set(projectId!, (ticketLoads.get(projectId!) ?? 0) + 1);
      return route.fulfill({ json: ticketPage(tickets) });
    }
    if (!path.startsWith('/__hotsheet/')) return route.continue();
    return route.fulfill({ status: 404, json: { error: `Unhandled ${request.method()} ${path}` } });
  });
  const socketFor = async (projectId: string) => {
    await expect.poll(() => sockets.get(projectId)?.length ?? 0).toBeGreaterThan(0);
    return sockets.get(projectId)!.at(-1)!;
  };
  /** Publish one change: to a routed (main-thread) socket when there is one, and to long polls. */
  const emit = (projectId: string, kind: string, slug = 'HS2-X') => {
    const next = (cursor.get(projectId) ?? 1) + 1;
    cursor.set(projectId, next);
    const change = { cursor: next, store: 'git', kind, id: 'ticket', slug };
    log.set(projectId, [...(log.get(projectId) ?? []), change]);
    sockets.get(projectId)?.at(-1)?.send(JSON.stringify(change));
    for (const waiting of held.get(projectId) ?? []) waiting.finish();
  };
  return { rows, emit, polls, sockets, socketFor, cursor, ticketLoads, held };
}

async function openBothProjects(page: Page) {
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByRole('button', { name: 'Add project' }).click();
  await expect(page.getByRole('tab', { name: 'Beta' })).toHaveAttribute('aria-selected', 'true');
  await expect(page.locator('[data-ticket-slug="HS2-BETA01"]')).toBeVisible();
  await page.getByRole('tab', { name: 'Alpha' }).click();
  await expect(page.locator('[data-ticket-slug="HS2-ALPHA1"]')).toBeVisible();
}

test('follows two projects through the shared change-stream worker during switching and search', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.addInitScript(() => {
    localStorage.setItem('hotsheet.change-stream-worker', 'true');
  });
  const workers: string[] = [];
  page.on('worker', (worker) => workers.push(worker.url()));
  const fixture = await installProjects(page);
  await openBothProjects(page);
  await expect.poll(() => workers.filter((url) => url.includes('change-stream')).length).toBe(1);

  // Sustained activity on both projects, interleaved with switching and searching.
  for (let index = 0; index < 20; index += 1) {
    fixture.emit(projects.alpha.id, 'activity');
    fixture.emit(projects.beta.id, 'activity');
  }
  fixture.rows[projects.beta.id] = [...fixture.rows[projects.beta.id], row('BETA02', 'Beta second')];
  fixture.emit(projects.beta.id, 'created', 'HS2-BETA02');
  await page.getByRole('tab', { name: 'Beta' }).click();
  await expect(page.locator('[data-ticket-slug="HS2-BETA02"]')).toBeVisible();
  fixture.rows[projects.alpha.id] = [...fixture.rows[projects.alpha.id], row('ALPHA2', 'Alpha second')];
  fixture.emit(projects.alpha.id, 'created', 'HS2-ALPHA2');
  await page.getByRole('tab', { name: 'Alpha' }).click();
  await expect(page.locator('[data-ticket-slug="HS2-ALPHA2"]')).toBeVisible();
  await page.getByRole('button', { name: 'Search tickets' }).click();
  await page.getByRole('searchbox', { name: 'Search tickets' }).fill('second');
  fixture.rows[projects.alpha.id] = fixture.rows[projects.alpha.id].map((ticket) =>
    ticket.id === 'ALPHA2' ? { ...ticket, title: 'Alpha second updated' } : ticket,
  );
  fixture.emit(projects.alpha.id, 'updated', 'HS2-ALPHA2');
  await expect(page.locator('[data-ticket-slug="HS2-ALPHA2"]')).toContainText('Alpha second updated');
  await page.getByRole('button', { name: 'Clear search' }).click();
  // The worker owns every change transport: the main thread opened no change socket of its own.
  expect([...fixture.sockets.values()].flat()).toEqual([]);

  // Idle: the worker's streams sit on pending long polls (this fixture refuses the worker's
  // socket upgrade), so the idle request rate stays bounded instead of looping.
  const idlePolls = fixture.polls.length;
  await page.waitForTimeout(3_000);
  expect(fixture.polls.length - idlePolls).toBeLessThanOrEqual(8);
  expect(fixture.held.get(projects.alpha.id)?.length ?? 0).toBeLessThanOrEqual(1);

  // Closing a project cancels its stream in the worker: its polls stop while the other continues.
  await page.getByRole('tab', { name: 'Beta' }).click();
  const betaPolls = () => fixture.polls.filter((poll) => poll.startsWith(`${projects.beta.id}?`)).length;
  await page.getByRole('button', { name: 'Close Beta' }).click();
  await page.getByRole('button', { name: 'Close Project' }).click();
  await expect(page.getByRole('tab', { name: 'Beta' })).toHaveCount(0);
  const afterClose = betaPolls();
  fixture.rows[projects.alpha.id] = [...fixture.rows[projects.alpha.id], row('ALPHA3', 'Alpha third')];
  fixture.emit(projects.alpha.id, 'created', 'HS2-ALPHA3');
  await expect(page.locator('[data-ticket-slug="HS2-ALPHA3"]')).toBeVisible();
  await page.waitForTimeout(1_500);
  expect(betaPolls()).toBe(afterClose);
});

test('keeps the main-thread stream when the worker setting is off', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  const workers: string[] = [];
  page.on('worker', (worker) => workers.push(worker.url()));
  const fixture = await installProjects(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByRole('button', { name: 'Add project' }).click();
  await expect(page.locator('[data-ticket-slug="HS2-BETA01"]')).toBeVisible();
  fixture.rows[projects.beta.id] = [...fixture.rows[projects.beta.id], row('BETA02', 'Beta second')];
  fixture.emit(projects.beta.id, 'created', 'HS2-BETA02');
  await expect(page.locator('[data-ticket-slug="HS2-BETA02"]')).toBeVisible();
  expect(workers.filter((url) => url.includes('change-stream'))).toEqual([]);
});

/** HS2-9XF60G: crash the bundled worker for real by throwing an uncaught error inside it. */
async function crashWorker(worker: Worker) {
  await worker.evaluate(() => {
    setTimeout(() => {
      throw new Error('Simulated change-stream worker crash (HS2-9XF60G)');
    }, 0);
  });
}
const streamWorkers = (workers: Worker[]) => workers.filter((worker) => worker.url().includes('change-stream'));
const enableWorker = (page: Page) =>
  page.addInitScript(() => {
    localStorage.setItem('hotsheet.change-stream-worker', 'true');
  });

test('a crashed worker restarts and replays from the last applied cursor without duplicates', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await enableWorker(page);
  const workers: Worker[] = [];
  page.on('worker', (worker) => workers.push(worker));
  const fixture = await installProjects(page);
  await openBothProjects(page);
  await expect.poll(() => streamWorkers(workers).length).toBe(1);
  fixture.rows[projects.alpha.id] = [...fixture.rows[projects.alpha.id], row('ALPHA2', 'Alpha second')];
  fixture.emit(projects.alpha.id, 'created', 'HS2-ALPHA2');
  await expect(page.locator('[data-ticket-slug="HS2-ALPHA2"]')).toBeVisible();
  const applied = { alpha: fixture.cursor.get(projects.alpha.id)!, beta: fixture.cursor.get(projects.beta.id)! };

  await crashWorker(streamWorkers(workers)[0]);
  // A change published while the worker is down must be replayed by its replacement.
  fixture.rows[projects.beta.id] = [...fixture.rows[projects.beta.id], row('BETA02', 'Beta second')];
  fixture.emit(projects.beta.id, 'created', 'HS2-BETA02');
  await expect.poll(() => streamWorkers(workers).length).toBe(2);
  const sinceOf = (projectId: string) =>
    fixture.polls
      .filter((poll) => poll.startsWith(`${projectId}?`))
      .map((poll) => new URLSearchParams(poll.slice(poll.indexOf('?') + 1)).get('since'));
  // The restarted worker resumes each project from what the main thread already applied.
  await expect.poll(() => sinceOf(projects.alpha.id)).toContain(String(applied.alpha));
  await expect.poll(() => sinceOf(projects.beta.id)).toContain(String(applied.beta));
  await page.getByRole('tab', { name: 'Beta' }).click();
  await expect(page.locator('[data-ticket-slug="HS2-BETA02"]')).toHaveCount(1);
  await page.getByRole('tab', { name: 'Alpha' }).click();
  await expect(page.locator('[data-ticket-slug="HS2-ALPHA2"]')).toHaveCount(1);
  // Replay repeats nothing: one new change costs at most one ticket reload.
  const alphaLoads = fixture.ticketLoads.get(projects.alpha.id) ?? 0;
  fixture.rows[projects.alpha.id] = [...fixture.rows[projects.alpha.id], row('ALPHA3', 'Alpha third')];
  fixture.emit(projects.alpha.id, 'created', 'HS2-ALPHA3');
  await expect(page.locator('[data-ticket-slug="HS2-ALPHA3"]')).toBeVisible();
  await page.waitForTimeout(500);
  expect((fixture.ticketLoads.get(projects.alpha.id) ?? 0) - alphaLoads).toBeLessThanOrEqual(1);
  // Still worker-owned: the main thread never opened its own change socket.
  expect([...fixture.sockets.values()].flat()).toEqual([]);
});

test('a crash-looping worker falls back to main-thread streams for both projects', async ({ page }) => {
  await page.setViewportSize({ width: 1024, height: 800 });
  await enableWorker(page);
  const workers: Worker[] = [];
  page.on('worker', (worker) => workers.push(worker));
  const fixture = await installProjects(page);
  await openBothProjects(page);
  // Four crashes inside the restart window exceed the three allowed restarts.
  for (let crash = 1; crash <= 4; crash += 1) {
    await expect.poll(() => streamWorkers(workers).length).toBe(crash);
    await crashWorker(streamWorkers(workers)[crash - 1]);
  }
  await fixture.socketFor(projects.alpha.id);
  await fixture.socketFor(projects.beta.id);
  await page.waitForTimeout(1_000);
  expect(streamWorkers(workers)).toHaveLength(4);
  fixture.rows[projects.beta.id] = [...fixture.rows[projects.beta.id], row('BETA02', 'Beta second')];
  fixture.emit(projects.beta.id, 'created', 'HS2-BETA02');
  fixture.rows[projects.alpha.id] = [...fixture.rows[projects.alpha.id], row('ALPHA2', 'Alpha second')];
  fixture.emit(projects.alpha.id, 'created', 'HS2-ALPHA2');
  await expect(page.locator('[data-ticket-slug="HS2-ALPHA2"]')).toBeVisible();
  await page.getByRole('tab', { name: 'Beta' }).click();
  await expect(page.locator('[data-ticket-slug="HS2-BETA02"]')).toBeVisible();
});

test('a version-skewed worker falls back to main-thread streams for both projects', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await enableWorker(page);
  await page.addInitScript(() => {
    // Simulate a stale cached worker bundle: its ready message advertises another protocol.
    const NativeWorker = window.Worker;
    window.Worker = class extends NativeWorker {
      override addEventListener(
        type: string,
        listener: EventListenerOrEventListenerObject | null,
        options?: boolean | AddEventListenerOptions,
      ) {
        if (!listener) return;
        if (type !== 'message' || typeof listener !== 'function') {
          super.addEventListener(type, listener, options);
          return;
        }
        super.addEventListener(
          type,
          (event) => {
            const data = event.data as { type?: string } | undefined;
            listener.call(
              this,
              data?.type === 'ready' ? new MessageEvent('message', { data: { ...data, protocol: 999 } }) : event,
            );
          },
          options,
        );
      }
    };
  });
  const workers: Worker[] = [];
  page.on('worker', (worker) => workers.push(worker));
  const fixture = await installProjects(page);
  await openBothProjects(page);
  await fixture.socketFor(projects.alpha.id);
  await fixture.socketFor(projects.beta.id);
  expect(streamWorkers(workers)).toHaveLength(1);
  fixture.rows[projects.alpha.id] = [...fixture.rows[projects.alpha.id], row('ALPHA2', 'Alpha second')];
  fixture.emit(projects.alpha.id, 'created', 'HS2-ALPHA2');
  await expect(page.locator('[data-ticket-slug="HS2-ALPHA2"]')).toBeVisible();
  fixture.rows[projects.beta.id] = [...fixture.rows[projects.beta.id], row('BETA02', 'Beta second')];
  fixture.emit(projects.beta.id, 'created', 'HS2-BETA02');
  await page.getByRole('tab', { name: 'Beta' }).click();
  await expect(page.locator('[data-ticket-slug="HS2-BETA02"]')).toBeVisible();
});
