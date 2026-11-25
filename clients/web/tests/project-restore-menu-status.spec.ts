import { expect, type Page, test, type WebSocketRoute } from '@playwright/test';

import { appRequests } from './app-requests';
import { capabilities } from './mobile-project-fixture';

// HS2-R64JXA: Restore Project rows draw the project tab's own status. The shipped app restores three
// remembered projects (a selected local one, a local one with active work, and a remote one) and
// keeps the last two minimized, so their rows must show the work badge and the cloud.
const BINDING = '01ARZ3NDEKTSV4RRFFQ69G5FAV';
const REMOTE_ID = `remote:${BINDING}:launch-checkout`;
const projects = [
  { id: 'alpha-project', root: '/work/alpha', name: 'Alpha' },
  { id: 'beta-project', root: '/work/beta', name: 'Beta' },
  { id: REMOTE_ID, root: REMOTE_ID, name: 'Build host · Launch', remoteHost: 'Build host' },
].map((item) => ({
  ...item,
  stores: item.remoteHost ? [] : [`${item.root}.hs2`],
  apiPath: `/__hotsheet/project-api/${encodeURIComponent(item.id)}`,
}));

const ticket = (id: string, title: string, extra: Record<string, unknown> = {}) => ({
  connection_id: 'git',
  native_id: id,
  qualified_id: `git:${id}`,
  id,
  slug: `HS2-${id}`,
  title,
  category: 'task',
  priority: 'default',
  status: 'not_started',
  up_next: true,
  feedback_needed: false,
  tags: [],
  blocked_by: [],
  claim_count: 0,
  created_at: '2026-11-01T00:00:00Z',
  updated_at: '2026-11-01T00:01:00Z',
  ...extra,
});
const claimed = { claimed_by: 'synthetic-worker', claim_lease_expires_at: '2099-01-01T00:00:00Z' };
const rows: Record<string, ReturnType<typeof ticket>[]> = {
  'alpha-project': [ticket('ALPHA1', 'Alpha work')],
  // Two Up Next tickets, both claimed by live workers: the badge shows 2 with a two-segment ring.
  'beta-project': [ticket('BETA01', 'Beta one', claimed), ticket('BETA02', 'Beta two', claimed)],
  [REMOTE_ID]: [ticket('LAUNCH', 'Launch work', { up_next: false })],
};

function ticketPage(items: ReturnType<typeof ticket>[]) {
  const active = items.filter((item) => 'claimed_by' in item).length;
  return {
    items,
    counts: {
      total: items.length,
      queued: items.length,
      backlog: 0,
      archive: 0,
      trash: 0,
      open: items.length,
      up_next: items.filter((item) => item.up_next).length,
      active,
      started: 0,
      completed_today: 0,
      completion_trend: [],
    },
  };
}

async function installProjects(page: Page) {
  await page.addInitScript(
    ({ roots, minimized }) => {
      if (sessionStorage.getItem('hs2-r64jxa-seeded')) return;
      sessionStorage.setItem('hs2-r64jxa-seeded', 'true');
      localStorage.setItem('hotsheet.open-projects', JSON.stringify(roots));
      localStorage.setItem('hotsheet.layout.minimized-projects.v1', JSON.stringify(minimized));
    },
    { roots: projects.map((item) => item.root), minimized: ['beta-project', REMOTE_ID] },
  );
  const sockets = new Map<string, WebSocketRoute>();
  await page.routeWebSocket(/\/ws\/sync(?:\?.*)?$/, (socket) => {
    sockets.set(decodeURIComponent(new URL(socket.url()).pathname.match(/project-api\/([^/]+)/)?.[1] ?? ''), socket);
  });
  await page.route(appRequests, async (route) => {
    const request = route.request(),
      path = decodeURIComponent(new URL(request.url()).pathname);
    if (/^\/__hotsheet\/(?:remote\/)?projects\/open$/.test(path)) {
      const root = (request.postDataJSON() as { root: string }).root;
      const item = projects.find((candidate) => candidate.root === root);
      return item
        ? route.fulfill({ status: 201, json: item })
        : route.fulfill({ status: 404, json: { error: 'Unknown synthetic project' } });
    }
    if (path === '/__hotsheet/remotes') return route.fulfill({ json: [] });
    const item = projects.find((candidate) => path.startsWith(`/__hotsheet/project-api/${candidate.id}/`));
    if (!item) return path.startsWith('/__hotsheet/') ? route.fulfill({ status: 404, json: {} }) : route.continue();
    if (path.endsWith('/ws/poll')) return route.fulfill({ json: { cursor: 1, events: [], overflow: false } });
    if (path.endsWith('/providers'))
      return route.fulfill({
        json: [
          {
            connection_id: 'git',
            provider: 'git',
            display_name: 'Hot Sheet Git',
            locator: `${item.root}.hs2`,
            default: true,
            capabilities,
          },
        ],
      });
    if (path.endsWith('/repository/status'))
      return route.fulfill({
        json: { branch: 'main', ahead: 0, behind: 0, staged: 0, unstaged: 0, untracked: 0, conflicted: 0, clean: true },
      });
    if (path.endsWith('/terminal-settings')) return route.fulfill({ json: { inherit_global_shell_history: false } });
    if (path.endsWith('/tickets') && request.method() === 'GET')
      return route.fulfill({ json: ticketPage(rows[item.id] ?? []) });
    if (
      /\/(?:provider-connections|corrupt-tickets|commands|command-runs|views|terminals|permissions|connections|sessions)$/.test(
        path,
      )
    )
      return route.fulfill({ json: [] });
    return route.fulfill({ status: 404, json: { error: `Unhandled ${request.method()} ${path}` } });
  });
  /** A live claim event on a background project's stream, as a worker claiming there sends. */
  return async (projectId: string) => {
    await expect.poll(() => sockets.has(projectId)).toBe(true);
    sockets
      .get(projectId)!
      .send(JSON.stringify({ cursor: 2, store: 'git', kind: 'claimed', id: 'BETA01', slug: 'HS2-BETA01' }));
  };
}

// Retina-density captures, so the small badge and ring read in the review attachments.
test.use({ deviceScaleFactor: 2 });

test('HS2-R64JXA Restore Project rows show the tab work badge and the remote cloud', async ({ page }, testInfo) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  const claimEvent = await installProjects(page);
  await page.goto('/');
  const strip = page.locator('.project-tab-bar:not(.project-tab-bar--mobile)');
  await expect(strip.locator('[data-tab-kind="project"][data-project-id="alpha-project"]')).toBeVisible();
  await expect(strip.locator('[data-tab-kind="project"]')).toHaveCount(1);
  // Workers claim Beta's tickets while its tab is minimized; its stream keeps running (HS2-4Q9DVS).
  await claimEvent('beta-project');

  const trigger = strip.getByRole('button', { name: 'Minimized Projects' });
  await trigger.click();
  const menu = page.locator('[data-minimized-projects="true"] [role="menu"]');
  const beta = menu.locator('[role="menuitem"][data-project-id="beta-project"]');
  const launch = menu.locator(`[role="menuitem"][data-project-id="${REMOTE_ID}"]`);
  await expect(beta).toBeVisible();
  await expect(beta).toContainText('Beta');
  await expect(beta.locator('[data-lucide="folder"]')).toHaveCount(1);
  // The same Up Next badge and active-ticket ring the tab draws.
  await expect(beta.locator('.project-tab__work')).toHaveAttribute('aria-label', '2 Up Next tickets, 2 active tickets');
  await expect(beta.locator('.project-tab__work-count')).toHaveText('2');
  await expect(beta.locator('.project-tab__activity-ring')).toBeVisible();
  await expect(launch).toHaveAttribute('data-location', 'remote');
  await expect(launch).toContainText('Build host · Launch');
  await expect(launch.locator('[data-lucide="cloud"]')).toBeVisible();
  await expect(launch.locator('.project-tab__work')).toHaveCount(0);
  // The badge sits at the row's trailing edge, inside it and vertically centered on it.
  const badge = (await beta.locator('.project-tab__work').boundingBox())!,
    row = (await beta.boundingBox())!;
  expect(badge.x + badge.width).toBeLessThanOrEqual(row.x + row.width);
  expect(Math.abs(badge.y + badge.height / 2 - (row.y + row.height / 2))).toBeLessThanOrEqual(2);

  const capture = async (name: string) => {
    // The current-row highlight, as in the review capture, must stay inside the menu inset (HS2-7DWS3S).
    await beta.hover();
    await menu.screenshot({ path: testInfo.outputPath(`HS2-R64JXA-restore-menu-${name}.png`), animations: 'disabled' });
    await page.screenshot({ path: testInfo.outputPath(`HS2-R64JXA-page-${name}.png`), animations: 'disabled' });
  };
  await capture('wide-light');
  await page.emulateMedia({ colorScheme: 'dark' });
  await capture('wide-dark');
  await page.emulateMedia({ colorScheme: 'light' });
  for (const [label, width] of [
    ['medium', 960],
    ['narrow', 390],
    ['wide-return', 1440],
  ] as const) {
    await page.keyboard.press('Escape');
    await page.setViewportSize({ width, height: width === 390 ? 844 : 900 });
    await page.getByRole('button', { name: 'Minimized Projects' }).click();
    await expect(launch.locator('[data-lucide="cloud"]')).toBeVisible();
    await expect(beta.locator('.project-tab__work')).toBeVisible();
    await capture(label);
  }
  await page.emulateMedia({ colorScheme: 'dark' });
  await capture('wide-return-dark');

  // Restoring keeps the tab's own presentation: the remote tab leads with the cloud.
  await launch.click();
  await expect(
    strip.locator(`[data-tab-kind="project"][data-project-id="${REMOTE_ID}"] [data-lucide="cloud"]`),
  ).toBeVisible();
});
