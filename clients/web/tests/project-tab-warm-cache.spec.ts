import { expect, type Page, type Route, test, type WebSocketRoute } from '@playwright/test';

import { PROJECT_WARM_CACHE_CAPACITY } from '../src/project-warm-cache';

// HS2-AZZ9TF: recently used project tabs stay warm (tickets, commands, AI tool inventory, terminal
// snapshot) so switching is instant — no loading placeholder, busy bars, or "Loading AI tools" /
// "Preparing terminals" activity label — while a bounded LRU sends older projects back to a cold load.

const PROJECT_COUNT = PROJECT_WARM_CACHE_CAPACITY + 1;
const projectId = (index: number) => `p${index}`;
const projects = Array.from({ length: PROJECT_COUNT }, (_, index) => ({
  id: projectId(index),
  root: `/work/p${index}`,
  name: `P${index}`,
  stores: [`/work/p${index}.hs2`],
  apiPath: `/__hotsheet/project-api/${projectId(index)}`,
  needsTicketSetup: false,
  needsHs1Migration: false,
}));
const base = {
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
};
const row = (id: string) => ({
  ...base,
  native_id: id,
  qualified_id: `git:${id}`,
  id,
  slug: `HS2-${id}`,
  title: `Ticket ${id}`,
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
const aiTools = [
  {
    id: 'codex',
    display_name: 'Codex',
    default_model: 'gpt-6',
    models: [{ id: 'gpt-6', display_name: 'GPT-6', effort_levels: ['medium'] }],
  },
];

interface LoggedRequest {
  project: string;
  rest: string;
}

async function installWorkspace(page: Page) {
  const rows: Record<string, FixtureTicket[]> = Object.fromEntries(
    projects.map((item) => [item.id, [row(`${item.name}A`)]]),
  );
  const requests: LoggedRequest[] = [];
  const holds = new Set<string>();
  const held = new Map<string, Route>();
  const sockets = new Map<string, WebSocketRoute[]>(),
    cursor = new Map<string, number>();

  await page.addInitScript(
    ({ roots }) => {
      localStorage.setItem('hotsheet.open-projects', JSON.stringify(roots));
      localStorage.setItem('hotsheet.workspace.active-project-root.v1', roots[0]);
      localStorage.setItem('hotsheet.show-loading-activity', 'true');
      localStorage.setItem('hotsheet.terminals.drawer-open', 'true');
      // Record every moment the global busy bars or loading-activity label become visible.
      const log: string[] = [];
      (window as unknown as { __busyLog: string[] }).__busyLog = log;
      const inspect = () => {
        const bars = document.querySelector('[data-component="server-busy-bars"]');
        const label = document.querySelector('[data-component="server-busy-message"]');
        if (bars?.getAttribute('data-visible') === 'true') log.push(`bars:${label?.textContent ?? ''}`);
        if (label?.getAttribute('data-visible') === 'true') log.push(`label:${label.textContent}`);
      };
      new MutationObserver(inspect).observe(document, {
        subtree: true,
        childList: true,
        characterData: true,
        attributes: true,
        attributeFilter: ['data-visible'],
      });
    },
    { roots: projects.map((item) => item.root) },
  );

  await page.routeWebSocket('**/__hotsheet/project-api/*/ws/sync', (socket) => {
    const id = decodeURIComponent(new URL(socket.url()).pathname.match(/project-api\/([^/]+)/)?.[1] ?? '');
    sockets.set(id, [...(sockets.get(id) ?? []), socket]);
    socket.onClose(() => {
      sockets.set(
        id,
        (sockets.get(id) ?? []).filter((candidate) => candidate !== socket),
      );
    });
  });

  await page.route('**/*', async (route) => {
    const request = route.request(),
      url = new URL(request.url()),
      path = decodeURIComponent(url.pathname);
    if (path === '/__hotsheet/projects/open' && request.method() === 'POST') {
      const root = (request.postDataJSON() as { root: string }).root;
      return route.fulfill({ status: 201, json: projects.find((item) => item.root === root) ?? projects[0] });
    }
    const api = path.match(/\/__hotsheet\/project-api\/([^/]+)(\/.*)$/);
    if (!api) {
      if (path.startsWith('/__hotsheet/')) return route.fulfill({ json: [] });
      return route.continue();
    }
    const [, id, rest] = api;
    if (!rest.endsWith('/ws/poll')) requests.push({ project: id, rest });
    if (rest.endsWith('/ws/poll')) {
      cursor.set(id, cursor.get(id) ?? 1);
      return route.fulfill({ json: { cursor: cursor.get(id), events: [], overflow: false } });
    }
    if (rest.endsWith('/providers'))
      return route.fulfill({
        json: [
          {
            connection_id: 'git',
            provider: 'git',
            display_name: 'Hot Sheet git',
            locator: `/work/${id}.hs2`,
            default: true,
            capabilities,
          },
        ],
      });
    if (rest.endsWith('/ai-tools')) return route.fulfill({ json: aiTools });
    if (rest.endsWith('/ai-settings')) return route.fulfill({ json: { tool: 'codex', model: 'gpt-6' } });
    if (rest.endsWith('/repository/status'))
      return route.fulfill({
        json: { branch: 'main', ahead: 0, behind: 0, staged: 0, unstaged: 0, untracked: 0, conflicted: 0, clean: true },
      });
    if (rest.endsWith('/terminal-settings')) return route.fulfill({ json: { inherit_global_shell_history: false } });
    if (rest.endsWith('/tickets') && request.method() === 'GET') {
      if (holds.has(id)) {
        holds.delete(id);
        held.set(id, route);
        return;
      }
      return route.fulfill({ json: ticketPage(rows[id] ?? []) });
    }
    if (request.method() === 'GET') return route.fulfill({ json: [] });
    return route.fulfill({ status: 204 });
  });

  const tab = (index: number) =>
    page.locator(`[data-tab-kind="project"][data-project-id="${projectId(index)}"]`).getByRole('tab');
  const ticketRow = (slug: string) => page.locator(`[data-component="ticket-list-row"][data-ticket-slug="${slug}"]`);

  return {
    rows,
    requests,
    tab,
    ticketRow,
    loading: page.locator('.ticket-empty-state--loading'),
    requestsFor: (index: number, suffix: string) =>
      requests.filter((item) => item.project === projectId(index) && item.rest.endsWith(suffix)).length,
    hold: (index: number) => {
      holds.add(projectId(index));
    },
    release: async (index: number) => {
      const id = projectId(index);
      await expect.poll(() => held.has(id)).toBe(true);
      const route = held.get(id)!;
      held.delete(id);
      await route.fulfill({ json: ticketPage(rows[id]) });
    },
    busyLog: () => page.evaluate(() => [...(window as unknown as { __busyLog: string[] }).__busyLog]),
    clearBusyLog: () =>
      page.evaluate(() => {
        (window as unknown as { __busyLog: string[] }).__busyLog.length = 0;
      }),
    emit: async (index: number, kind: string) => {
      const id = projectId(index);
      await expect.poll(() => sockets.get(id)?.length ?? 0).toBeGreaterThan(0);
      const next = (cursor.get(id) ?? 1) + 1;
      cursor.set(id, next);
      sockets
        .get(id)!
        .at(-1)!
        .send(JSON.stringify({ cursor: next, store: 'git', kind, id: 'ticket', slug: `HS2-${projects[index].name}A` }));
    },
  };
}

type Workspace = Awaited<ReturnType<typeof installWorkspace>>;

async function bootWorkspace(page: Page, workspace: Workspace) {
  await page.goto('/?dev-review=false');
  await expect(workspace.tab(0)).toHaveAttribute('aria-selected', 'true');
  await expect(workspace.ticketRow('HS2-P0A')).toBeVisible();
  // Startup stays active-only (HS2-V9ZVW0): inactive projects load when first selected.
  for (let index = 1; index < PROJECT_COUNT; index += 1) expect(workspace.requestsFor(index, '/tickets')).toBe(0);
}

/** First visits are ordinary cold loads; they are what makes a project warm. */
async function warmUp(workspace: Workspace, indexes: number[]) {
  for (const index of indexes) {
    await switchTo(workspace, index);
    await expect(workspace.ticketRow(`HS2-P${index}A`)).toBeVisible();
    await expect.poll(() => workspace.requestsFor(index, '/ai-settings')).toBeGreaterThan(0);
  }
}

async function switchTo(workspace: Workspace, index: number) {
  await workspace.tab(index).click();
  await expect(workspace.tab(index)).toHaveAttribute('aria-selected', 'true');
}

test.beforeEach(async ({ page }) => {
  await page.setViewportSize({ width: 1600, height: 900 });
});

test('A→B→A switching between warm projects paints from cache without loading, busy bars, or AI/terminal refetches', async ({
  page,
}) => {
  const workspace = await installWorkspace(page);
  await bootWorkspace(page, workspace);
  await warmUp(workspace, [1, 2, 0]);
  await expect(page.locator('[data-component="server-busy-message"]')).toHaveAttribute('data-visible', 'false');
  await workspace.clearBusyLog();
  const aiBefore = workspace.requests.filter((item) => /\/ai-(tools|settings)$/.test(item.rest)).length,
    terminalsBefore = workspace.requests.filter((item) => item.rest.endsWith('/terminals')).length;

  for (const index of [1, 0, 1, 2, 1, 0]) {
    await switchTo(workspace, index);
    // Instant paint: the cached rows are present in the same frame the tab becomes selected.
    await expect(workspace.ticketRow(`HS2-P${index}A`)).toBeVisible({ timeout: 250 });
    await expect(workspace.loading).toHaveCount(0);
  }
  await page.waitForTimeout(600);
  expect(workspace.requests.filter((item) => /\/ai-(tools|settings)$/.test(item.rest)).length).toBe(aiBefore);
  expect(workspace.requests.filter((item) => item.rest.endsWith('/terminals')).length).toBe(terminalsBefore);
  expect(await workspace.busyLog()).toEqual([]);
  await expect(page.locator('[data-component="server-busy-message"]')).toHaveAttribute('data-visible', 'false');

  // Visual evidence (HS2-AZZ9TF): the frame right after a warm switch, at wide and narrow widths.
  const shots = process.env.HS2_AZZ9TF_SHOTS;
  if (shots) {
    await workspace.tab(1).click();
    await page.screenshot({ path: `${shots}/warm-switch-wide.png` });
    await page.setViewportSize({ width: 760, height: 800 });
    await expect(workspace.ticketRow('HS2-P1A')).toBeVisible();
    await page.waitForTimeout(600); // let the responsive sidebar transition settle
    await page.screenshot({ path: `${shots}/warm-switch-narrow.png` });
  }
});

test('a project evicted beyond the warm LRU bound reloads with a loading state; warm ones stay instant', async ({
  page,
}) => {
  const workspace = await installWorkspace(page);
  await bootWorkspace(page, workspace);
  const cold = PROJECT_WARM_CACHE_CAPACITY;
  // Visit P1..P7 so the warm set is full and P0 becomes its least recently used project.
  await warmUp(
    workspace,
    Array.from({ length: cold - 1 }, (_, index) => index + 1),
  );
  // The never-visited project is cold: it shows the loading state until its tickets arrive.
  workspace.hold(cold);
  await switchTo(workspace, cold);
  await expect(workspace.loading).toBeVisible();
  await workspace.release(cold);
  await expect(workspace.ticketRow(`HS2-P${cold}A`)).toBeVisible();
  await expect(workspace.loading).toHaveCount(0);

  // Visiting it evicted P0 (least recently used): P0 now reloads cold.
  const aiBefore = workspace.requestsFor(0, '/ai-tools');
  workspace.hold(0);
  await switchTo(workspace, 0);
  await expect(workspace.loading).toBeVisible();
  await workspace.release(0);
  await expect(workspace.ticketRow('HS2-P0A')).toBeVisible();
  await expect.poll(() => workspace.requestsFor(0, '/ai-tools')).toBeGreaterThan(aiBefore);
  // Let the cold reload (and its visible busy indicator) settle before asserting warm switches.
  await expect.poll(() => workspace.requestsFor(0, '/ai-settings')).toBeGreaterThan(aiBefore);
  await expect(page.locator('[data-component="server-busy-message"]')).toHaveAttribute('data-visible', 'false');

  // Recently used projects are still warm and instant.
  await workspace.clearBusyLog();
  for (const index of [cold, cold - 1, 0]) {
    await switchTo(workspace, index);
    await expect(workspace.ticketRow(`HS2-P${index}A`)).toBeVisible({ timeout: 250 });
    await expect(workspace.loading).toHaveCount(0);
  }
  await page.waitForTimeout(600);
  expect(await workspace.busyLog()).toEqual([]);
});

test('live change events keep a cached background project current so its switch shows fresh rows immediately', async ({
  page,
}) => {
  const workspace = await installWorkspace(page);
  await bootWorkspace(page, workspace);
  await warmUp(workspace, [1, 0]);

  const before = workspace.requestsFor(1, '/tickets');
  workspace.rows.p1 = [row('P1A'), row('P1LIVE')];
  await workspace.emit(1, 'created');
  await expect.poll(() => workspace.requestsFor(1, '/tickets')).toBeGreaterThan(before);
  await expect(workspace.tab(1).locator('..').locator('.project-tab__work')).toHaveAttribute(
    'aria-label',
    '2 Up Next tickets',
  );

  // Hold the activation's silent revalidation: the live-updated cache alone must already show the new row.
  workspace.hold(1);
  await workspace.clearBusyLog();
  await switchTo(workspace, 1);
  await expect(workspace.ticketRow('HS2-P1LIVE')).toBeVisible({ timeout: 250 });
  await expect(workspace.loading).toHaveCount(0);
  await workspace.release(1);
  await expect(workspace.ticketRow('HS2-P1LIVE')).toBeVisible();
  await page.waitForTimeout(600);
  expect(await workspace.busyLog()).toEqual([]);
});

test('rapid switching and leaving a cold load in flight never shows stale rows or strands a loading state', async ({
  page,
}) => {
  const workspace = await installWorkspace(page);
  await bootWorkspace(page, workspace);
  await warmUp(workspace, [1, 2, 0]);
  const cold = PROJECT_WARM_CACHE_CAPACITY;

  // Leave the cold project while its first load is still in flight.
  workspace.hold(cold);
  await switchTo(workspace, cold);
  await expect(workspace.loading).toBeVisible();
  await switchTo(workspace, 1);
  await expect(workspace.ticketRow('HS2-P1A')).toBeVisible({ timeout: 250 });
  await expect(workspace.loading).toHaveCount(0);
  await workspace.release(cold);
  await page.waitForTimeout(300);
  // The late cold response must not leak into the active warm project.
  await expect(workspace.ticketRow(`HS2-P${cold}A`)).toHaveCount(0);
  await expect(workspace.ticketRow('HS2-P1A')).toBeVisible();

  // An abandoned cold activation must not have cached an empty projection: returning loads it properly.
  await switchTo(workspace, cold);
  await expect(workspace.ticketRow(`HS2-P${cold}A`)).toBeVisible();
  await expect(page.locator('.ticket-empty-state--project')).toHaveCount(0);

  // Hammer A/B/C switching without waiting for any revalidation to settle.
  for (const index of [0, 1, 2, 0, 2, 1, 0, 1]) await workspace.tab(index).click();
  await expect(workspace.tab(1)).toHaveAttribute('aria-selected', 'true');
  await expect(workspace.ticketRow('HS2-P1A')).toBeVisible();
  await expect(workspace.ticketRow('HS2-P0A')).toHaveCount(0);
  await expect(workspace.ticketRow('HS2-P2A')).toHaveCount(0);
  await expect(workspace.loading).toHaveCount(0);
});
