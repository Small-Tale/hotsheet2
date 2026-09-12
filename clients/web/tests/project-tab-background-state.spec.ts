import { expect, type Page, type Route, test } from '@playwright/test';

const projects = {
  alpha: { id: 'alpha-project', root: '/work/alpha', name: 'Alpha', stores: ['/work/alpha.hs2'], apiPath: '/__hotsheet/project-api/alpha-project' },
  beta: { id: 'beta-project', root: '/work/beta', name: 'Beta', stores: ['/work/beta.hs2'], apiPath: '/__hotsheet/project-api/beta-project' },
} as const;
const base = { connection_id: 'git', category: 'bug', priority: 'default', status: 'not_started', feedback_needed: false, tags: [], blocked_by: [], claim_count: 0, created_at: '2026-09-11T00:00:00Z', updated_at: '2026-09-11T00:01:00Z' };
const row = (id: string, project: string, upNext = true) => ({ ...base, native_id: id, qualified_id: `git:${id}`, id, slug: `HS2-${id}`, title: `${project} ${id}`, up_next: upNext });
type FixtureTicket = ReturnType<typeof row> & { claimed_by?: string; worker_label?: string; claim_lease_expires_at?: string };

async function installProjects(page: Page) {
  const rows: Record<string, FixtureTicket[]> = {
    'alpha-project': [row('ALPHA1', 'Alpha')],
    'beta-project': [row('BETA01', 'Beta')],
  };
  const pollWaiters = new Map<string, Route[]>();
  const handshaken = new Set<string>();
  const cursor = new Map<string, number>();
  let holdBetaRefresh = false;
  let heldBetaTickets: Route | undefined;

  await page.route('**/*', async route => {
    const request = route.request(), url = new URL(request.url()), path = decodeURIComponent(url.pathname);
    if (path === '/__hotsheet/projects/open') {
      const root = (request.postDataJSON() as { root: string }).root;
      return route.fulfill({ status: 201, json: root === projects.beta.root ? projects.beta : projects.alpha });
    }
    if (path === '/__hotsheet/folders/choose') return route.fulfill({ json: { path: projects.beta.root } });
    const projectId = path.match(/project-api\/([^/]+)/)?.[1], tickets = projectId ? rows[projectId] ?? [] : [];
    if (path.endsWith('/ws/poll') && projectId) {
      if (!handshaken.has(projectId)) {
        handshaken.add(projectId);
        cursor.set(projectId, 1);
        return route.fulfill({ json: { cursor: 1, events: [], overflow: false } });
      }
      pollWaiters.set(projectId, [...pollWaiters.get(projectId) ?? [], route]);
      return;
    }
    if (path.endsWith('/providers')) return route.fulfill({ json: [{ connection_id: 'git', provider: 'git', display_name: 'Hot Sheet git', locator: `/work/${projectId}.hs2`, default: true, capabilities: { create: true, update: true, close: true, notes: true, note_edit: true, note_delete: true, attachments: true, assignment: true, review_requests: true, dependencies: true, up_next: true, close_reasons: true, claims: true, atomic_batch: true, not_working_report: true, offline_mutation: true, history: true, watch: true, provider_idempotency: true, query_fields: [] } }] });
    if (path.endsWith('/provider-connections') || path.endsWith('/corrupt-tickets') || path.endsWith('/commands') || path.endsWith('/command-runs') || path.endsWith('/views') || path.endsWith('/terminals') || path.endsWith('/permissions') || path.endsWith('/connections') || path.endsWith('/drive/sessions')) return route.fulfill({ json: [] });
    if (path.endsWith('/repository/status')) return route.fulfill({ json: { branch: 'main', ahead: 0, behind: 0, staged: 0, unstaged: 0, untracked: 0, conflicted: 0, clean: true } });
    if (path.endsWith('/terminal-settings')) return route.fulfill({ json: { inherit_global_shell_history: false } });
    if (path.endsWith('/tickets') && request.method() === 'GET') {
      if (projectId === projects.beta.id && holdBetaRefresh) {
        heldBetaTickets = route;
        holdBetaRefresh = false;
        return;
      }
      return route.fulfill({ json: tickets });
    }
    const ticketId = path.match(/\/tickets\/([^/]+)$/)?.[1];
    if (ticketId && request.method() === 'GET') {
      const ticket = tickets.find(item => item.id === ticketId);
      return ticket ? route.fulfill({ json: { store: 'git', ...ticket, blocked_reason: null, concurrency_token: `token-${ticket.id}`, notes: [], attachments: [] } }) : route.fulfill({ status: 404, json: { error: 'Ticket not found' } });
    }
    if (!path.startsWith('/__hotsheet/')) return route.continue();
    return route.fulfill({ status: 404, json: { error: `Unhandled ${request.method()} ${path}` } });
  });

  const emit = async (projectId: string, kind: string) => {
    await expect.poll(() => pollWaiters.get(projectId)?.length ?? 0).toBeGreaterThan(0);
    const [waiter, ...remaining] = pollWaiters.get(projectId)!;
    pollWaiters.set(projectId, remaining);
    const next = (cursor.get(projectId) ?? 1) + 1;
    cursor.set(projectId, next);
    await waiter.fulfill({ json: { cursor: next, events: [{ store: 'git', kind, id: 'ticket', slug: 'HS2-BETA01' }], overflow: false } });
  };

  return {
    rows,
    emit,
    holdNextBetaRefresh: () => { holdBetaRefresh = true; },
    finishHeldBetaRefresh: async () => {
      await expect.poll(() => Boolean(heldBetaTickets)).toBe(true);
      const held = heldBetaTickets!;
      heldBetaTickets = undefined;
      await held.fulfill({ json: rows[projects.beta.id] });
    },
  };
}

async function openBothProjects(page: Page) {
  await page.goto('/?dev-review=false');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByRole('button', { name: 'Add project' }).click();
  await expect(page.getByRole('tab', { name: 'Beta' })).toHaveAttribute('aria-selected', 'true');
  await expect(page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-BETA01"]')).toBeVisible();
  await page.getByRole('tab', { name: 'Alpha' }).click();
  await expect(page.getByRole('tab', { name: 'Alpha' })).toHaveAttribute('aria-selected', 'true');
  await expect(page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-ALPHA1"]')).toBeVisible();
  await expect(page.locator('[data-ticket-motion-ghost]')).toHaveCount(0);
}

test('refreshes non-active project tab counts and live-work state while preserving cache-fast switching', async ({ page }) => {
  await page.setViewportSize({ width: 1200, height: 800 });
  const fixture = await installProjects(page);
  await openBothProjects(page);
  const alphaTab = page.locator('[data-component="project-tab"][data-project-id="alpha-project"]');
  const betaTab = page.locator('[data-component="project-tab"][data-project-id="beta-project"]');
  await expect(betaTab.locator('.project-tab__work')).toHaveAttribute('aria-label', '1 Up Next ticket');

  fixture.rows[projects.beta.id] = [
    { ...row('BETA01', 'Beta'), claimed_by: 'codex-beta', worker_label: 'Codex Beta', claim_lease_expires_at: '2099-09-11T12:00:00Z' },
    row('BETA02', 'Beta'),
  ];
  await fixture.emit(projects.beta.id, 'claimed');
  await expect(betaTab.locator('.project-tab__work')).toHaveAttribute('aria-label', '2 Up Next tickets, 1 active ticket');
  await expect(betaTab.locator('.project-tab__activity-ring')).toBeVisible();
  await expect(alphaTab.getByRole('tab')).toHaveAttribute('aria-selected', 'true');
  await expect(page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-ALPHA1"]')).toBeVisible();
  await expect(page.locator('[data-ticket-motion-ghost]')).toHaveCount(0);
  await page.screenshot({ path: '/private/tmp/hs2-qbnrzy-background-project-tab-wide.png', fullPage: true });

  await page.setViewportSize({ width: 1024, height: 700 });
  await expect(betaTab.locator('.project-tab__work-count')).toHaveText('1');
  await page.screenshot({ path: '/private/tmp/hs2-qbnrzy-background-project-tab-narrow.png', fullPage: true });

  fixture.holdNextBetaRefresh();
  await betaTab.getByRole('tab').click();
  await expect(betaTab.getByRole('tab')).toHaveAttribute('aria-selected', 'true');
  await expect(page.locator('[data-ticket-slug="HS2-BETA01"]')).toBeVisible();
  await expect(page.locator('[data-ticket-slug="HS2-BETA02"]')).toBeVisible();
  await expect(page.getByRole('status', { name: 'Loading' })).toHaveCount(0);
  await fixture.finishHeldBetaRefresh();

  await alphaTab.getByRole('tab').click();
  fixture.rows[projects.beta.id] = [row('BETA01', 'Beta')];
  await fixture.emit(projects.beta.id, 'released');
  await expect(betaTab.locator('.project-tab__work')).toHaveAttribute('aria-label', '1 Up Next ticket');
  await expect(betaTab.locator('.project-tab__activity-ring')).toHaveCount(0);
  await expect(alphaTab.getByRole('tab')).toHaveAttribute('aria-selected', 'true');
});
