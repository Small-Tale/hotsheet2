import { expect, type Page, test } from '@playwright/test';

import type { LiveStoreState } from '../src/api';
import { ticketsJson } from './ticket-page-fixture';

test.use({ deviceScaleFactor: 2 });

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

const MINUTE = 60_000;

/** A live snapshot in the server's exact shape (HS2-JTTQMY); synthetic names only. */
function liveStore(id: string, checkout: string, now: number, overrides: Partial<LiveStoreState> = {}): LiveStoreState {
  return {
    store: id,
    root: `/work/${id}.hs2`,
    name: id,
    checkouts: [checkout],
    epoch_ms: 1,
    revision: 1,
    state: 'active',
    activity: {
      word: 'active',
      active_since_ms: now - 10 * MINUTE,
      stopped_at_ms: null,
      last_progress_ms: now - 2 * MINUTE,
      idle_deadline_ms: null,
      stalled: [],
      halted: [],
      causes: [],
    },
    claims: [
      {
        ticket: `${id}-t`,
        slug: `${id.toUpperCase()}-1`,
        title: 'Synthetic work',
        worker: 'w1',
        phase: 'working',
        claimed_at_ms: now - 10 * MINUTE,
        lease_expires_at_ms: now + 20 * MINUTE,
        eta_at_ms: null,
      },
    ],
    workers: { live: 1, expected: 1 },
    waiting: { count: 0, tickets: [] },
    runway: { ready: 0, estimate_ms: null },
    rates: {
      tokens: {
        window_minutes: 5,
        minutes: [0, 1, 2, 3, 4].map((ago) => ({ start_ms: now - ago * MINUTE, tokens: 900 })),
        peak_per_minute: 2000,
      },
      tickets_updated: { window_minutes: 60, at_ms: [now - MINUTE, now - 5 * MINUTE], peak_per_hour: 6 },
    },
    ...overrides,
  };
}

/** Two open projects through the production shell, with mocked server routes and live state. */
async function openWorkspaceGrid(page: Page) {
  await page.addInitScript(() => {
    localStorage.setItem('hotsheet.open-projects', JSON.stringify(['/work/demo', '/work/other', '/work/quiet']));
    localStorage.setItem('hotsheet.workspace.active-project-root.v1', '/work/demo');
  });
  await page.route('**/*', async (route) => {
    const request = route.request(),
      url = new URL(request.url()),
      path = url.pathname,
      now = Date.now();
    if (path === '/__hotsheet/projects/open') {
      const root = request.postDataJSON().root as string,
        id = `${root.split('/').at(-1)}-checkout`;
      return route.fulfill({
        status: 201,
        json: {
          id,
          root,
          name: root.split('/').at(-1),
          stores: [`${root}.hs2`],
          apiPath: `/__hotsheet/project-api/${id}`,
        },
      });
    }
    if (path.endsWith('/live-state'))
      return route.fulfill({
        json: {
          epoch_ms: 1,
          stores: [
            liveStore('demo', 'demo-checkout', now),
            liveStore('other', 'other-checkout', now, {
              state: 'may_be_stuck',
              activity: { ...liveStore('other', 'other-checkout', now).activity, last_progress_ms: now - 34 * MINUTE },
            }),
            liveStore('quiet', 'quiet-checkout', now, {
              state: 'stopped',
              claims: [],
              workers: { live: 0, expected: 0 },
              activity: {
                ...liveStore('quiet', 'quiet-checkout', now).activity,
                word: 'stopped',
                stopped_at_ms: now - 120 * MINUTE,
              },
            }),
          ],
        },
      });
    if (path.endsWith('/providers'))
      return route.fulfill({
        json: [
          {
            connection_id: 'git-local',
            provider: 'git',
            display_name: 'Hot Sheet Git',
            locator: '/tickets',
            default: true,
            capabilities,
          },
        ],
      });
    if (path.endsWith('/tickets')) return route.fulfill({ json: ticketsJson(request, []) });
    if (
      path.endsWith('/terminals') ||
      path.endsWith('/permissions') ||
      path.endsWith('/connections') ||
      path.endsWith('/commands') ||
      path.endsWith('/command-runs') ||
      path.endsWith('/corrupt-tickets')
    )
      return route.fulfill({ json: [] });
    if (path.endsWith('/repository/status'))
      return route.fulfill({
        json: { branch: 'main', ahead: 0, behind: 0, staged: 0, unstaged: 0, untracked: 0, conflicted: 0, clean: true },
      });
    if (path.endsWith('/ws/poll'))
      return route.fulfill({
        json: { cursor: Number(url.searchParams.get('since') ?? 0), events: [], overflow: false },
      });

    return route.continue();
  });
  await page.goto('/');
  await expect(page.locator('[data-component="app-shell"]')).toBeVisible({ timeout: 10_000 });
  await page.getByRole('button', { name: 'Workspace Grid' }).click();
  const sidebar = page.locator('#app-left-rail');
  const pager = sidebar.locator('#grid-sidebar-pages');
  await expect(pager).toBeVisible();
  return { sidebar, pager };
}

const storedPage = (page: Page) => page.evaluate(() => localStorage.getItem('hotsheet.terminals.sidebar-page'));
const currentPanel = (pager: ReturnType<Page['locator']>) => pager.locator('[role="tabpanel"]:not([inert])');
/** How far the current page sits from the viewport's start edge; 0 once the pager has settled. */
const pageOffset = (pager: ReturnType<Page['locator']>) =>
  pager.locator('.kui-paged-view__viewport').evaluate((node) => {
    const view = node.parentElement!;
    const target = node.querySelector<HTMLElement>(`[data-paged-view-page="${view.dataset.page}"]`)!;
    return Math.abs(target.getBoundingClientRect().left - node.getBoundingClientRect().left);
  });

test('pages the workspace grid sidebar by dot, arrow key, and trackpad scroll and remembers the page (HS2-EZSQDF)', async ({
  page,
}) => {
  // The app's minimum height (600px) is where a long page must scroll under the dots.
  await page.setViewportSize({ width: 1440, height: 600 });
  const { sidebar, pager } = await openWorkspaceGrid(page);
  // Completion is the default page.
  await expect(pager).toHaveAttribute('data-page', 'completion');
  await expect(currentPanel(pager).locator('[data-component="project-summary"]')).toHaveCount(4);
  const dots = pager.getByRole('tablist', { name: 'Sidebar pages' });
  await expect(dots.getByRole('tab')).toHaveCount(3);
  // Dot click: the gauge page, with live gauges and the state glyph per project.
  await dots.getByRole('tab', { name: 'Page 2 of 3: Gauges' }).click();
  await expect(pager).toHaveAttribute('data-page', 'gauges');
  await expect.poll(() => storedPage(page)).toBe('gauges');
  const gauges = currentPanel(pager);
  await expect(gauges.locator('[data-component="grid-sidebar-gauges"]')).toHaveCount(4);
  await expect(gauges.locator('[data-project-id="other-checkout"] [data-live-word="may_be_stuck"]')).toBeVisible();
  await expect(gauges.locator('[data-project-id="other-checkout"] [data-live-word="may_be_stuck"] title')).toHaveText(
    /no progress for 34 minutes/,
  );
  await expect(gauges.locator('[data-project-id="all"] [data-live-word]')).toHaveCount(0);
  await expect(gauges.locator('[data-variant="bare"][data-empty="false"]')).not.toHaveCount(0);
  await page.screenshot({ path: 'target/visual-captures/hs2-ezsqdf-gauges-1440.png' });

  // The dots stay fixed at the sidebar bottom while the page scrolls underneath them.
  const before = await dots.boundingBox();
  const scroller = currentPanel(pager);
  const overflow = await scroller.evaluate((node) => node.scrollHeight - node.clientHeight);
  expect(overflow).toBeGreaterThan(0);
  await scroller.evaluate((node) => {
    node.scrollTop = node.scrollHeight;
  });
  await expect.poll(() => scroller.evaluate((node) => node.scrollTop)).toBeGreaterThan(0);
  expect(await dots.boundingBox()).toEqual(before);
  const sidebarBox = (await sidebar.boundingBox())!;
  expect(before!.y + before!.height).toBeLessThanOrEqual(sidebarBox.y + sidebarBox.height);
  expect(before!.y + before!.height).toBeGreaterThan(sidebarBox.y + sidebarBox.height - 24);

  // Arrow key on the focused dot: the triage page, sorted by need.
  await dots.getByRole('tab', { name: 'Page 2 of 3: Gauges' }).press('ArrowRight');
  await expect(pager).toHaveAttribute('data-page', 'triage');
  await expect(dots.getByRole('tab', { name: 'Page 3 of 3: Triage' })).toBeFocused();
  const triage = currentPanel(pager);
  await expect
    .poll(() =>
      triage
        .locator('[data-grid-sidebar-group]')
        .evaluateAll((groups) => groups.map((group) => group.getAttribute('data-project-id'))),
    )
    .toEqual(['all', 'other-checkout', 'demo-checkout', 'quiet-checkout']);
  await expect(triage.locator('[data-live-word="stopped"]')).toContainText('Stopped');
  await expect(triage.locator('[data-live-word="may_be_stuck"]')).toContainText('May be stuck');
  await expect(triage.locator('[data-live-word="may_be_stuck"]')).toContainText('OTHER-1 · working');
  await expect(triage.getByText('34 minutes')).toHaveCount(0);
  await page.screenshot({ path: 'target/visual-captures/hs2-ezsqdf-triage-1440.png' });

  // A horizontal trackpad scroll settles on the previous page. The arrow key updates data-page at
  // once while the viewport is still smooth-scrolling to triage; a wheel delivered mid-animation is
  // absorbed by that scroll's snap, so wait for the pager to settle first (HS2-24XBNR).
  await expect.poll(() => pageOffset(pager)).toBeLessThan(1);
  const viewportBox = (await pager.locator('.kui-paged-view__viewport').boundingBox())!;
  await page.mouse.move(viewportBox.x + viewportBox.width / 2, viewportBox.y + 80);
  await page.mouse.wheel(-viewportBox.width, 0);
  await expect(pager).toHaveAttribute('data-page', 'gauges');
  await expect.poll(() => storedPage(page)).toBe('gauges');

  // The page persists for this viewer across a reload.
  await page.reload();
  await expect(page.locator('[data-component="app-shell"]')).toBeVisible({ timeout: 10_000 });
  await page.getByRole('button', { name: 'Workspace Grid' }).click();
  await expect(page.locator('#grid-sidebar-pages')).toHaveAttribute('data-page', 'gauges');
  await expect(
    currentPanel(page.locator('#grid-sidebar-pages')).locator('[data-component="grid-sidebar-gauges"]'),
  ).toHaveCount(4);
});

test('jumps between grid sidebar pages without animation under reduced motion (HS2-EZSQDF)', async ({ page }) => {
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await page.setViewportSize({ width: 1280, height: 800 });
  const { pager } = await openWorkspaceGrid(page);
  const viewport = pager.locator('.kui-paged-view__viewport');
  await expect(viewport).toHaveCSS('scroll-behavior', 'auto');
  await pager.getByRole('tab', { name: 'Page 3 of 3: Triage' }).click();
  await expect(pager).toHaveAttribute('data-page', 'triage');
  // With no smooth scroll, the viewport is already on the triage page.
  await expect
    .poll(() =>
      viewport.evaluate((node) => {
        const target = node.querySelector<HTMLElement>('[data-paged-view-page="triage"]')!;
        return Math.abs(target.getBoundingClientRect().left - node.getBoundingClientRect().left);
      }),
    )
    .toBeLessThan(1);
});

for (const colorScheme of ['light', 'dark'] as const) {
  test(`keeps the grid sidebar pages aligned wide to narrow to wide in ${colorScheme} (HS2-KBRZTN)`, async ({
    page,
  }) => {
    await page.emulateMedia({ colorScheme });
    await page.setViewportSize({ width: 1440, height: 900 });
    const { sidebar, pager } = await openWorkspaceGrid(page);
    for (const [width, label] of [
      [1440, 'wide'],
      [1024, 'medium'],
      [1440, 'wide-again'],
    ] as const) {
      await page.setViewportSize({ width, height: 900 });
      for (const name of ['Completion', 'Gauges', 'Triage']) {
        await pager.getByRole('tab', { name: new RegExp(`: ${name}$`) }).click();
        await expect(pager).toHaveAttribute('data-page', name.toLowerCase());
        // The current page is snapped into the view, not half-scrolled after a resize.
        await expect
          .poll(() =>
            pager.locator('.kui-paged-view__viewport').evaluate((node) => {
              const view = node.parentElement as HTMLElement;
              const target = node.querySelector<HTMLElement>(`[data-paged-view-page="${view.dataset.page}"]`)!;
              return Math.abs(target.getBoundingClientRect().left - node.getBoundingClientRect().left);
            }),
          )
          .toBeLessThan(1);
        // No horizontal overflow inside a page.
        expect(await currentPanel(pager).evaluate((node) => node.scrollWidth <= node.clientWidth)).toBe(true);
        await page.screenshot({
          path: `target/visual-captures/hs2-ezsqdf-${name.toLowerCase()}-${label}-${colorScheme}.png`,
        });
      }
    }
    // Narrow: the phone layout opens the operations sidebar as an overlay; the dots stay in it.
    await page.setViewportSize({ width: 390, height: 844 });
    const show = page.getByRole('button', { name: 'Show Operations Sidebar' });
    await expect(show).toBeVisible();
    await show.click();
    await expect(sidebar).not.toHaveAttribute('aria-hidden', 'true');
    await expect(pager.getByRole('tablist', { name: 'Sidebar pages' })).toBeInViewport();
    await pager.getByRole('tab', { name: /: Gauges$/ }).click();
    await expect(pager).toHaveAttribute('data-page', 'gauges');
    await expect(pager.getByRole('tab', { name: /: Gauges$/ })).toHaveAttribute('aria-selected', 'true');
    await expect.poll(() => pageOffset(pager)).toBeLessThan(1);
    expect(await currentPanel(pager).evaluate((node) => node.scrollWidth <= node.clientWidth)).toBe(true);
    await page.screenshot({ path: `target/visual-captures/hs2-ezsqdf-gauges-narrow-${colorScheme}.png` });
  });
}
