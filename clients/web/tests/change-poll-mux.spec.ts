import { expect, type Page, test } from '@playwright/test';

/**
 * HS2-6W1NNF: with WebSockets failing, every open project used to hold its own replay long-poll,
 * so six or more projects filled the browser's per-origin HTTP/1.1 connection pool. Projects
 * whose bridge advertises the multiplexed change poll now share one long-poll request.
 */
const MUX = '/__hotsheet/change-mux/poll';
const names = ['Alpha', 'Bravo', 'Charlie', 'Delta', 'Echo', 'Foxtrot', 'Golf', 'Hotel'];
const projects = names.map((name) => ({
  id: `${name.toLowerCase()}-project`,
  root: `/work/${name.toLowerCase()}`,
  name,
  stores: [`/work/${name.toLowerCase()}.hs2`],
  apiPath: `/__hotsheet/project-api/${name.toLowerCase()}-project`,
  changeMuxPath: MUX,
}));
const row = (id: string, title: string) => ({
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
  up_next: true,
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
    up_next: tickets.length,
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

interface MuxPoll {
  project: string;
  since?: number;
  timeout_ms: number;
}

async function installBridge(page: Page) {
  const rows = new Map(
    projects.map((project) => [project.id, [row(`${project.name.toUpperCase()}1`, `${project.name} first`)]]),
  );
  const cursor = new Map<string, number>(projects.map((project) => [project.id, 1]));
  const log = new Map<string, Array<{ cursor: number; store: string; kind: string; id: string; slug: string }>>();
  const wakers = new Set<() => void>();
  const muxBodies: MuxPoll[][] = [];
  const directPolls: string[] = [];
  const ticketLoads = new Map<string, number>();
  let chosen = 0;
  const eventsAfter = (project: string, since: number) =>
    (log.get(project) ?? []).filter((item) => item.cursor > since);
  // Every change transport fails its WebSocket upgrade, as behind a proxy that rejects upgrades.
  await page.routeWebSocket(/.*/, (socket) => {
    void socket.close();
  });
  await page.route('**/*', async (route) => {
    const request = route.request(),
      url = new URL(request.url()),
      path = decodeURIComponent(url.pathname);
    if (path === '/__hotsheet/projects/open') {
      const root = (request.postDataJSON() as { root: string }).root;
      return route.fulfill({ status: 201, json: projects.find((project) => project.root === root) ?? projects[0] });
    }
    if (path === '/__hotsheet/folders/choose') {
      chosen += 1;
      return route.fulfill({ json: { path: projects[chosen].root } });
    }
    if (path === MUX) {
      const polls = (request.postDataJSON() as { polls: MuxPoll[] }).polls;
      muxBodies.push(polls);
      const ready = () =>
        polls.filter(
          (poll) =>
            poll.since === undefined || poll.timeout_ms === 0 || eventsAfter(poll.project, poll.since).length > 0,
        );
      // A genuine multiplexed long poll: idle, it stays pending until an event or a bounded timeout.
      if (ready().length === 0)
        await new Promise<void>((resolve) => {
          const wake = () => {
            wakers.delete(wake);
            resolve();
          };
          wakers.add(wake);
          setTimeout(wake, Math.min(2_000, ...polls.map((poll) => poll.timeout_ms)));
        });
      const answered = ready().length ? ready() : polls;
      return route
        .fulfill({
          json: {
            version: 1,
            results: answered.map((poll) => ({
              project: poll.project,
              status: 200,
              body: {
                cursor: cursor.get(poll.project),
                events: poll.since === undefined ? [] : eventsAfter(poll.project, poll.since),
                overflow: false,
              },
            })),
          },
        })
        .catch(() => undefined);
    }
    const projectId = path.match(/project-api\/([^/]+)/)?.[1];
    if (path.endsWith('/ws/poll')) {
      directPolls.push(path);
      return route.fulfill({ json: { cursor: 1, events: [], overflow: false } });
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
      [
        '/provider-connections',
        '/corrupt-tickets',
        '/commands',
        '/command-runs',
        '/views',
        '/terminals',
        '/permissions',
        '/connections',
        '/drive/sessions',
      ].some((suffix) => path.endsWith(suffix))
    )
      return route.fulfill({ json: [] });
    if (path.endsWith('/repository/status'))
      return route.fulfill({
        json: { branch: 'main', ahead: 0, behind: 0, staged: 0, unstaged: 0, untracked: 0, conflicted: 0, clean: true },
      });
    if (path.endsWith('/terminal-settings')) return route.fulfill({ json: { inherit_global_shell_history: false } });
    if (path.endsWith('/tickets') && request.method() === 'GET' && projectId) {
      ticketLoads.set(projectId, (ticketLoads.get(projectId) ?? 0) + 1);
      return route.fulfill({ json: ticketPage(rows.get(projectId) ?? []) });
    }
    if (!path.startsWith('/__hotsheet/')) return route.continue();
    return route.fulfill({ status: 404, json: { error: `Unhandled ${request.method()} ${path}` } });
  });
  /** Track how many multiplexed polls the browser holds open at once. */
  let open = 0,
    maxOpen = 0;
  const isMux = (url: string) => new URL(url).pathname === MUX;
  page.on('request', (request) => {
    if (!isMux(request.url())) return;
    open += 1;
    maxOpen = Math.max(maxOpen, open);
  });
  const settle = (request: { url(): string }) => {
    if (isMux(request.url())) open -= 1;
  };
  page.on('requestfinished', settle);
  page.on('requestfailed', settle);
  const emit = (project: string, kind: string, slug: string) => {
    const next = (cursor.get(project) ?? 1) + 1;
    cursor.set(project, next);
    log.set(project, [...(log.get(project) ?? []), { cursor: next, store: 'git', kind, id: slug, slug }]);
    for (const wake of [...wakers]) wake();
  };
  return { rows, emit, muxBodies, directPolls, ticketLoads, maxOpen: () => maxOpen, open: () => open };
}

async function openAllProjects(page: Page) {
  await page.goto('/');
  await page.getByRole('button', { name: 'Open Project' }).click();
  await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();
  await expect(page.getByRole('tab', { name: projects[0].name })).toBeVisible();
  for (const project of projects.slice(1)) {
    await page.getByRole('button', { name: 'Add Project' }).click();
    await expect(page.getByRole('tab', { name: project.name })).toHaveAttribute('aria-selected', 'true');
  }
}

for (const mode of ['main thread', 'worker'] as const)
  test(`eight projects with failing WebSockets share one change long-poll (${mode})`, async ({ page }) => {
    test.setTimeout(120_000);
    await page.setViewportSize({ width: 1440, height: 900 });
    if (mode === 'worker')
      await page.addInitScript(() => {
        localStorage.setItem('hotsheet.change-stream-worker', 'true');
      });
    const bridge = await installBridge(page);
    await openAllProjects(page);

    // Every project's stream rides the multiplexed poll; none falls back to its own long poll.
    // (A project between its WebSocket retry and its replay poll is briefly absent from the
    // current request, so collect across requests and require that requests are shared.)
    await expect
      .poll(() => new Set(bridge.muxBodies.flatMap((polls) => polls.map((poll) => poll.project))).size, {
        timeout: 20_000,
      })
      .toBe(projects.length);
    await expect
      .poll(() => Math.max(...bridge.muxBodies.map((polls) => polls.length)), { timeout: 20_000 })
      .toBeGreaterThanOrEqual(4);
    expect(bridge.directPolls).toEqual([]);

    // Idle: the browser holds at most one shared request (two while one is being replaced), and
    // the rate stays bounded. Each project's failed WebSocket retry still joins the shared poll on
    // its own backoff schedule, which starts at 500 ms and doubles, so early on several joins
    // land per second; a tight loop would be hundreds.
    const idleStart = bridge.muxBodies.length;
    await page.waitForTimeout(3_000);
    expect(bridge.muxBodies.length - idleStart).toBeLessThanOrEqual(30);
    expect(bridge.maxOpen()).toBeLessThanOrEqual(2);

    // An event for one background project reaches that project only.
    const target = projects[3];
    const loadsBefore = new Map(bridge.ticketLoads);
    bridge.rows.set(target.id, [...bridge.rows.get(target.id)!, row('DELTA2', 'Delta second')]);
    bridge.emit(target.id, 'created', 'HS2-DELTA2');
    await expect
      .poll(() => (bridge.ticketLoads.get(target.id) ?? 0) - (loadsBefore.get(target.id) ?? 0), { timeout: 10_000 })
      .toBeGreaterThan(0);
    await page.getByRole('tab', { name: target.name }).click();
    await expect(page.locator('[data-ticket-slug="HS2-DELTA2"]')).toBeVisible();
    for (const project of projects.filter(
      (candidate) => candidate.id !== target.id && candidate.id !== projects.at(-1)!.id,
    ))
      expect(bridge.ticketLoads.get(project.id) ?? 0).toBe(loadsBefore.get(project.id) ?? 0);

    // An ordinary request is answered at once while the shared poll is held.
    const started = Date.now();
    await page.getByRole('tab', { name: projects[1].name }).click();
    await expect(page.locator(`[data-ticket-slug="HS2-${projects[1].name.toUpperCase()}1"]`)).toBeVisible();
    expect(Date.now() - started).toBeLessThan(5_000);

    // Closing a project removes it from the shared poll while the others continue.
    const closing = projects[2];
    await page.getByRole('tab', { name: closing.name }).click();
    await page.getByRole('button', { name: `Close ${closing.name}` }).click();
    await page.getByRole('button', { name: 'Close Project' }).click();
    await expect(page.getByRole('tab', { name: closing.name })).toHaveCount(0);
    await expect
      .poll(() => bridge.muxBodies.at(-1)?.some((poll) => poll.project === closing.id), { timeout: 10_000 })
      .toBe(false);
    // Each remaining project still polls through the shared request (a project between its
    // WebSocket retry and its replay poll is briefly absent, so collect across requests).
    const afterClose = bridge.muxBodies.length;
    bridge.emit(projects[0].id, 'activity', 'HS2-ALPHA1');
    await expect
      .poll(() => new Set(bridge.muxBodies.slice(afterClose).flatMap((polls) => polls.map((poll) => poll.project))), {
        timeout: 40_000,
      })
      .toEqual(new Set(projects.filter((project) => project.id !== closing.id).map((project) => project.id)));
    expect(bridge.directPolls).toEqual([]);
    expect(bridge.maxOpen()).toBeLessThanOrEqual(2);
  });

test('an older bridge without the multiplexed poll keeps per-project long polls', async ({ page }) => {
  test.setTimeout(60_000);
  await page.setViewportSize({ width: 1440, height: 900 });
  const directPolls: string[] = [];
  let muxRequests = 0;
  await page.routeWebSocket(/.*/, (socket) => {
    void socket.close();
  });
  await page.route('**/*', async (route) => {
    const request = route.request(),
      path = decodeURIComponent(new URL(request.url()).pathname);
    if (path === '/__hotsheet/projects/open') return route.fulfill({ status: 201, json: projects[0] });
    // The older bridge has no such route.
    if (path === MUX) {
      muxRequests += 1;
      return route.fulfill({ status: 404, body: 'Not Found' });
    }
    if (path.endsWith('/ws/poll')) {
      directPolls.push(path);
      await new Promise((resolve) => setTimeout(resolve, 500));
      return route.fulfill({ json: { cursor: 1, events: [], overflow: false } }).catch(() => undefined);
    }
    if (path.endsWith('/providers'))
      return route.fulfill({
        json: [
          {
            connection_id: 'git',
            provider: 'git',
            display_name: 'Hot Sheet Git',
            locator: '/work/alpha.hs2',
            default: true,
            capabilities,
          },
        ],
      });
    if (path.endsWith('/tickets') && request.method() === 'GET')
      return route.fulfill({ json: ticketPage([row('ALPHA1', 'Alpha first')]) });
    if (path.endsWith('/repository/status'))
      return route.fulfill({
        json: { branch: 'main', ahead: 0, behind: 0, staged: 0, unstaged: 0, untracked: 0, conflicted: 0, clean: true },
      });
    if (path.endsWith('/terminal-settings')) return route.fulfill({ json: { inherit_global_shell_history: false } });
    if (path.startsWith('/__hotsheet/project-api/')) return route.fulfill({ json: [] });
    if (!path.startsWith('/__hotsheet/')) return route.continue();
    return route.fulfill({ status: 404, json: { error: `Unhandled ${path}` } });
  });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open Project' }).click();
  await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();
  await expect(page.locator('[data-ticket-slug="HS2-ALPHA1"]')).toBeVisible();
  await expect.poll(() => directPolls.length, { timeout: 10_000 }).toBeGreaterThan(0);
  // The 404 is learned once; the bridge is not asked again.
  await page.waitForTimeout(2_000);
  expect(muxRequests).toBe(1);
});
