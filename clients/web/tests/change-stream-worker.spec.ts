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
  // Projects whose stream handshake (a poll without `since`) is held until the test releases it (HS2-SQKJYG).
  const handshakeGates = new Map<string, { arrived: boolean; open: Promise<void>; release: () => void }>();
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
    if (/^\/__hotsheet\/(?:remote\/)?projects\/open$/.test(path)) {
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
      const gate = sinceParam === null ? handshakeGates.get(projectId) : undefined;
      if (gate) {
        gate.arrived = true;
        await gate.open;
        handshakeGates.delete(projectId);
        return respond();
      }
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
            display_name: 'Hot Sheet Git',
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
      // Like the server, name the change cursor the read was served at (HS2-SQKJYG).
      return route.fulfill({
        json: ticketPage(tickets),
        headers: { 'x-hotsheet-change-cursor': String(cursor.get(projectId!) ?? 1) },
      });
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
  /** Hold a project's next stream handshake; `arrived` reports it is pending, `release` answers it. */
  const holdHandshake = (projectId: string) => {
    let release!: () => void;
    const open = new Promise<void>((resolve) => {
      release = resolve;
    });
    const gate = { arrived: false, open, release };
    handshakeGates.set(projectId, gate);
    return { arrived: () => gate.arrived, release };
  };
  return { rows, emit, polls, sockets, socketFor, cursor, ticketLoads, held, holdHandshake };
}

async function openBothProjects(page: Page) {
  await page.goto('/');
  await page.getByRole('button', { name: 'Open Project' }).click();
  await page.getByRole('button', { name: 'Local Project' }).click();
  await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();
  await page.getByRole('button', { name: 'Add Project' }).click();
  await page.getByRole('button', { name: 'Local Project' }).click();
  await expect(page.getByRole('tab', { name: 'Beta' })).toHaveAttribute('aria-selected', 'true');
  await expect(page.locator('[data-ticket-slug="HS2-BETA01"]')).toBeVisible();
  await page.getByRole('tab', { name: 'Alpha' }).click();
  await expect(page.locator('[data-ticket-slug="HS2-ALPHA1"]')).toBeVisible();
}

/** Open both projects with their streams in one dedicated change-stream worker. */
async function openBothProjectsInWorker(page: Page) {
  await page.addInitScript(() => {
    localStorage.setItem('hotsheet.change-stream-worker', 'true');
    localStorage.setItem('hotsheet.change-stream-shared-worker', 'false');
    // See `enableWorker` (HS2-5598QK).
    localStorage.setItem('hotsheet.change-stream-worker-ready-timeout-ms', '30000');
  });
  const workers: string[] = [];
  page.on('worker', (worker) => workers.push(worker.url()));
  const fixture = await installProjects(page);
  await openBothProjects(page);
  await expect.poll(() => workers.filter((url) => url.includes('change-stream')).length).toBe(1);
  return fixture;
}

test('follows two projects through the shared change-stream worker during switching and search', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  const fixture = await openBothProjectsInWorker(page);

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
});

// Split from the switching scenario so neither long flow nears the test budget under heavy parallel load (HS2-5598QK).
test('the shared change-stream worker idles on bounded long polls and stops a closed project', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  const fixture = await openBothProjectsInWorker(page);
  // Both streams follow a cursor before the idle baseline, so their handshakes are not counted as idle polls.
  await expect
    .poll(() =>
      [projects.alpha.id, projects.beta.id].every((id) =>
        fixture.polls.some(
          (poll) => poll.startsWith(`${id}?`) && new URLSearchParams(poll.slice(poll.indexOf('?') + 1)).has('since'),
        ),
      ),
    )
    .toBe(true);

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
  await page.getByRole('button', { name: 'Open Project' }).click();
  await page.getByRole('button', { name: 'Local Project' }).click();
  await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();
  await page.getByRole('button', { name: 'Add Project' }).click();
  await page.getByRole('button', { name: 'Local Project' }).click();
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
    localStorage.setItem('hotsheet.change-stream-shared-worker', 'false');
    // The dev server compiles the worker's module graph on demand, which under heavy parallel load
    // can exceed the bundled worker's 2 s ready timeout and silently fall back to main-thread streams.
    // These scenarios are about a worker that did start, so wait for its real `ready` (HS2-5598QK).
    localStorage.setItem('hotsheet.change-stream-worker-ready-timeout-ms', '30000');
  });

for (const mode of ['main-thread', 'worker'] as const)
  test(`a ${mode} stream applies a change made between the ticket load and its handshake (HS2-SQKJYG)`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1280, height: 900 });
    if (mode === 'worker') await enableWorker(page);
    const workers: Worker[] = [];
    page.on('worker', (worker) => workers.push(worker));
    const fixture = await installProjects(page);
    const beta = projects.beta.id,
      handshake = fixture.holdHandshake(beta);
    await page.goto('/');
    await page.getByRole('button', { name: 'Open Project' }).click();
    await page.getByRole('button', { name: 'Local Project' }).click();
    await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();
    await page.getByRole('button', { name: 'Add Project' }).click();
    await page.getByRole('button', { name: 'Local Project' }).click();
    await expect(page.locator('[data-ticket-slug="HS2-BETA01"]')).toBeVisible();
    // Beta's tickets were read and its stream's handshake is pending: the window is open.
    await expect.poll(handshake.arrived).toBe(true);
    const loadsBefore = fixture.ticketLoads.get(beta) ?? 0;
    fixture.rows[beta] = [...fixture.rows[beta], row('BETA02', 'Beta second')];
    fixture.emit(beta, 'created', 'HS2-BETA02');
    // The handshake answers with the cursor that already includes the change, so the stream never
    // delivers it: the read's stamp (1) is below the handshake (2), and only that gap is replayed.
    handshake.release();
    await expect(page.locator('[data-ticket-slug="HS2-BETA02"]')).toBeVisible();
    await expect(page.locator('[data-ticket-slug="HS2-BETA02"]')).toHaveCount(1);
    expect(fixture.polls.filter((poll) => poll.startsWith(`${beta}?timeout_ms=0&since=1&`))).toHaveLength(1);
    if (mode === 'worker') {
      expect(streamWorkers(workers)).toHaveLength(1);
      expect([...fixture.sockets.values()].flat(), 'the worker owns the streams').toEqual([]);
    } else expect(streamWorkers(workers)).toEqual([]);
    // Reconciled once; idle time reads no tickets and holds a bounded number of change polls.
    expect((fixture.ticketLoads.get(beta) ?? 0) - loadsBefore).toBeGreaterThanOrEqual(1);
    await expect
      .poll(() =>
        fixture.polls.some(
          (poll) => poll.startsWith(`${beta}?`) && new URLSearchParams(poll.slice(poll.indexOf('?') + 1)).has('since'),
        ),
      )
      .toBe(true);
    const idleLoads = new Map(fixture.ticketLoads),
      idlePolls = fixture.polls.length;
    await page.waitForTimeout(3_000);
    expect(fixture.ticketLoads).toEqual(idleLoads);
    expect(fixture.polls.length - idlePolls).toBeLessThanOrEqual(8);
    // A later change still arrives exactly once: the reconcile did not disturb the cursor.
    fixture.rows[beta] = [...fixture.rows[beta], row('BETA03', 'Beta third')];
    fixture.emit(beta, 'created', 'HS2-BETA03');
    await expect(page.locator('[data-ticket-slug="HS2-BETA03"]')).toBeVisible();
    await expect(page.locator('[data-ticket-slug="HS2-BETA02"]')).toHaveCount(1);
  });

test('a crashed worker restarts and replays from the last applied cursor without duplicates', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await enableWorker(page);
  const workers: Worker[] = [];
  page.on('worker', (worker) => workers.push(worker));
  const fixture = await installProjects(page);
  await openBothProjects(page);
  await expect.poll(() => streamWorkers(workers).length).toBe(1);
  const sinceOf = (projectId: string) =>
    fixture.polls
      .filter((poll) => poll.startsWith(`${projectId}?`))
      .map((poll) => new URLSearchParams(poll.slice(poll.indexOf('?') + 1)).get('since'));
  const loads = (projectId: string) => fixture.ticketLoads.get(projectId) ?? 0;
  // A worker that has spawned may not have subscribed yet: a change published before a project's stream
  // handshake is not part of this scenario (HS2-4FGEQ5), so wait until both streams follow a cursor.
  await expect
    .poll(() => [projects.alpha.id, projects.beta.id].every((id) => sinceOf(id).some((since) => since !== null)))
    .toBe(true);
  fixture.rows[projects.alpha.id] = [...fixture.rows[projects.alpha.id], row('ALPHA2', 'Alpha second')];
  fixture.emit(projects.alpha.id, 'created', 'HS2-ALPHA2');
  await expect(page.locator('[data-ticket-slug="HS2-ALPHA2"]')).toBeVisible();
  const applied = { alpha: fixture.cursor.get(projects.alpha.id)!, beta: fixture.cursor.get(projects.beta.id)! };
  // The scenario needs a worker that became ready: one that misses the host's ready timeout falls back to
  // main-thread sockets for good, and crashing it would test nothing (HS2-4FGEQ5).
  expect([...fixture.sockets.values()].flat(), 'the worker still owns both streams before the crash').toEqual([]);

  await crashWorker(streamWorkers(workers)[0]);
  // A change published while the worker is down must be replayed by its replacement.
  fixture.rows[projects.beta.id] = [...fixture.rows[projects.beta.id], row('BETA02', 'Beta second')];
  fixture.emit(projects.beta.id, 'created', 'HS2-BETA02');
  await expect.poll(() => streamWorkers(workers).length).toBe(2);
  // The restarted worker resumes each project from what the main thread already applied.
  await expect.poll(() => sinceOf(projects.alpha.id)).toContain(String(applied.alpha));
  await expect.poll(() => sinceOf(projects.beta.id)).toContain(String(applied.beta));
  await page.getByRole('tab', { name: 'Beta' }).click();
  await expect(page.locator('[data-ticket-slug="HS2-BETA02"]')).toHaveCount(1);
  // Selecting a project tab reloads its tickets once. Its rows are already rendered, so wait for that
  // reload itself before taking the baseline; on a slow runner it otherwise lands after the baseline and
  // is charged to the next change (HS2-4FGEQ5).
  const beforeSwitch = loads(projects.alpha.id);
  await page.getByRole('tab', { name: 'Alpha' }).click();
  await expect.poll(() => loads(projects.alpha.id)).toBeGreaterThan(beforeSwitch);
  await expect(page.locator('[data-ticket-slug="HS2-ALPHA2"]')).toHaveCount(1);
  // Replay repeats nothing: one new change costs at most one ticket reload.
  const alphaLoads = loads(projects.alpha.id);
  fixture.rows[projects.alpha.id] = [...fixture.rows[projects.alpha.id], row('ALPHA3', 'Alpha third')];
  fixture.emit(projects.alpha.id, 'created', 'HS2-ALPHA3');
  await expect(page.locator('[data-ticket-slug="HS2-ALPHA3"]')).toBeVisible();
  // A later change is the settle point: a project's deliveries apply in order, so once it renders, any
  // repeated ALPHA3 delivery has already been applied and counted. Two changes, at most two reloads.
  fixture.rows[projects.alpha.id] = [...fixture.rows[projects.alpha.id], row('ALPHA4', 'Alpha fourth')];
  fixture.emit(projects.alpha.id, 'created', 'HS2-ALPHA4');
  await expect(page.locator('[data-ticket-slug="HS2-ALPHA4"]')).toBeVisible();
  expect(loads(projects.alpha.id) - alphaLoads).toBeLessThanOrEqual(2);
  await expect(page.locator('[data-ticket-slug="HS2-ALPHA3"]')).toHaveCount(1);
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

test('a worker-owned bridge socket relays the host client build to the newer-version notice (HS2-WHW3M1)', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await enableWorker(page);
  const workers: Worker[] = [];
  page.on('worker', (worker) => workers.push(worker));
  await installProjects(page);
  const MUX = '/__hotsheet/change-mux/poll';
  const STAMPED = 'bundle-0123456789abcdef';
  // Registered after the fixture, so these run first: the projects advertise the change bridge, and
  // the document is stamped with another bundle than the dev bridge serves.
  await page.route('**/*', async (route) => {
    const request = route.request(),
      path = new URL(request.url()).pathname;
    if (request.resourceType() === 'document') {
      const response = await route.fetch();
      const html = (await response.text()).replace(
        /(<meta name="hotsheet-client-build" content=")[^"]*(")/u,
        `$1${STAMPED}$2`,
      );
      return route.fulfill({ response, body: html });
    }
    if (/^\/__hotsheet\/(?:remote\/)?projects\/open$/.test(path)) {
      const root = (request.postDataJSON() as { root: string }).root;
      return route.fulfill({
        status: 201,
        json: { ...(root === projects.beta.root ? projects.beta : projects.alpha), changeMuxPath: MUX },
      });
    }
    if (path === MUX) {
      const polls = (request.postDataJSON() as { polls: { project: string; since?: number; timeout_ms: number }[] })
        .polls;
      // A genuine long poll: idle, it stays pending until its bounded timeout.
      if (polls.every((poll) => poll.since !== undefined && poll.timeout_ms > 0))
        await new Promise((resolve) => setTimeout(resolve, Math.min(2_000, ...polls.map((poll) => poll.timeout_ms))));
      return route
        .fulfill({
          json: {
            version: 1,
            results: polls.map((poll) => ({
              project: poll.project,
              status: 200,
              body: { cursor: 1, events: [], overflow: false },
            })),
          },
        })
        .catch(() => undefined);
    }
    return route.fallback();
  });
  await openBothProjects(page);
  expect(await page.locator('meta[name="hotsheet-client-build"]').getAttribute('content')).toBe(STAMPED);
  await expect.poll(() => streamWorkers(workers).length).toBe(1);
  // The worker's socket reaches the real dev bridge, which has no session for these projects: its
  // `closed` answer carries the dev server's build, which the worker relays to this page.
  const notice = page.locator('[data-component="client-update-notice-banner"]');
  await expect(notice).toBeVisible();
  await expect(notice).toContainText('A newer version of Hot Sheet is available');
  await page.screenshot({
    path: 'target/visual-captures/hs2-whw3m1-worker-client-update-notice-1280.png',
    animations: 'disabled',
  });
  // Still worker-owned: no fallback started main-thread project sockets or a second worker.
  expect(streamWorkers(workers)).toHaveLength(1);
});
