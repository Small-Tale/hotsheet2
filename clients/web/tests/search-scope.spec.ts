import { expect, type Locator, type Page, test } from '@playwright/test';

// Search scope across the built-in views: searching from Trash searches only Trash (HS2-S6TXSW), and
// any other search can switch into All Tickets from its floating toolbar (HS2-4J6764).

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
const project = {
  id: 'demo',
  root: '/work/demo',
  name: 'Demo',
  stores: ['/work/demo.hs2'],
  apiPath: '/__hotsheet/project-api/demo',
  needsTicketSetup: false,
  needsHs1Migration: false,
  hs1ImportCompleted: false,
  hs1CleanupEligible: false,
};
const row = (slug: string, title: string, status: string) => ({
  connection_id: 'git-local',
  native_id: slug,
  qualified_id: `git-local:${slug}`,
  id: slug,
  slug,
  title,
  category: 'feature',
  priority: 'default',
  status,
  up_next: false,
  feedback_needed: false,
  tags: [],
  blocked_by: [],
  claim_count: 0,
  created_at: '2026-09-01T00:00:00Z',
  updated_at: '2026-09-14T00:00:00Z',
});
const rows = [
  row('HS2-QUEUE1', 'Badge spacing in the queue', 'started'),
  row('HS2-QUEUE2', 'Unrelated queue work', 'not_started'),
  row('HS2-DONE01', 'Badge contrast shipped', 'completed'),
  row('HS2-BACK01', 'Badge color backlog idea', 'backlog'),
  row('HS2-BACK02', 'Unrelated backlog idea', 'backlog'),
  row('HS2-ARCH01', 'Badge archived decision', 'archive'),
  row('HS2-TRASH1', 'Badge count trashed', 'deleted'),
  row('HS2-TRASH2', 'Unrelated trashed one', 'deleted'),
];

function inScope(ticket: (typeof rows)[number], url: URL) {
  const status = url.searchParams.get('status'),
    collection = url.searchParams.get('collection');
  if (status) return ticket.status === status;
  if (collection === 'trash') return ticket.status === 'deleted';
  if (collection === 'archive') return ticket.status === 'archive';
  if (collection === 'all') return ticket.status !== 'deleted';
  return !['backlog', 'archive', 'deleted'].includes(ticket.status);
}

/** Mock one project whose ticket endpoint scopes rows the way the server does, recording each search scope. */
async function mockSearchProject(page: Page, views: Array<{ id: string; name: string; query: string }> = []) {
  const searches: string[] = [];
  await page.route('**/*', async (route) => {
    const request = route.request(),
      url = new URL(request.url()),
      path = url.pathname;
    if (path === '/__hotsheet/projects/open') return route.fulfill({ status: 201, json: project });
    if (path === '/__hotsheet/folders/choose') return route.fulfill({ json: { path: '/work/demo' } });
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
    // HS2-CSP42A: the real inspector/reader parent requests a qualified ticket detail.
    const ticketId = decodeURIComponent(path).match(/\/tickets\/([^/]+)$/)?.[1];
    if (ticketId && request.method() === 'GET') {
      const ticket = rows.find((item) => [item.id, item.native_id, item.qualified_id].includes(ticketId));
      return ticket
        ? route.fulfill({
            json: {
              ...ticket,
              store: ticket.connection_id,
              details: 'Synthetic search overlay ticket details.',
              blocked_reason: null,
              concurrency_token: `token-${ticket.id}`,
              notes: [],
              attachments: [],
            },
          })
        : route.fulfill({ status: 404, json: { error: 'Synthetic ticket not found' } });
    }
    if (path.endsWith('/tickets') && request.method() === 'GET') {
      const text = (url.searchParams.get('text') ?? '').toLowerCase();
      if (text) searches.push(url.searchParams.get('status') ?? url.searchParams.get('collection') ?? 'queue');
      const items = rows.filter((ticket) => inScope(ticket, url) && ticket.title.toLowerCase().includes(text));
      return route.fulfill({
        json: {
          items,
          counts: {
            total: 6,
            queued: 3,
            backlog: 2,
            archive: 1,
            trash: 2,
            open: 2,
            up_next: 0,
            active: 1,
            started: 1,
            completed_today: 0,
            completion_trend: [0, 0, 0, 0, 0, 0, 0],
          },
        },
      });
    }
    if (path.endsWith('/views')) return route.fulfill({ json: views });
    if (path.endsWith('/repository/status'))
      return route.fulfill({
        json: { branch: 'main', ahead: 0, behind: 0, staged: 0, unstaged: 0, untracked: 0, conflicted: 0, clean: true },
      });
    if (path.endsWith('/ws/poll')) {
      if (url.searchParams.get('since') === null)
        return route.fulfill({ json: { cursor: 1, events: [], overflow: false } });
      return;
    }
    if (
      path.endsWith('/terminals') ||
      path.endsWith('/connections') ||
      path.endsWith('/commands') ||
      path.endsWith('/command-runs') ||
      path.endsWith('/corrupt-tickets')
    )
      return route.fulfill({ json: [] });
    return route.continue();
  });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open Project' }).click();
  await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();
  await expect(page.locator('[data-ticket-slug="HS2-QUEUE1"]').first()).toBeVisible();
  return searches;
}

/** The sidebar view navigation, opening the overlay sidebar first at narrow widths. */
async function viewNavigation(page: Page) {
  const navigation = page.getByRole('navigation', { name: 'Ticket Views' });
  if (!(await navigation.isVisible())) await page.getByRole('button', { name: 'Show Project Sidebar' }).click();
  await expect(navigation).toBeVisible();
  return navigation;
}
/** Close the overlay sidebar at narrow widths so it no longer covers the workspace. */
async function closeOverlaySidebar(page: Page) {
  const hide = page.getByRole('button', { name: 'Hide Project Sidebar' });
  if (
    (await page.locator('#app-left-rail').getAttribute('data-presentation')) === 'overlay' &&
    (await hide.isVisible())
  )
    await hide.click();
}
const viewCount = (navigation: Locator, view: string) =>
  navigation.getByRole('button', { name: new RegExp(`^${view}`) }).locator('.view-navigation__count');

/** HS2-CSP42A: visibility/focus alone passes even when the compact rail paints over search. */
async function expectReachableWorkspaceSearch(page: Page) {
  const search = page.getByRole('searchbox', { name: 'Search tickets' });
  await expect(search).toBeVisible();
  await expect(search).toBeFocused();
  await expect
    .poll(() =>
      search.evaluate((node) => {
        const box = node.getBoundingClientRect(),
          x = box.x + box.width / 2,
          y = box.y + box.height / 2;
        if (box.width <= 0 || box.height <= 0 || x < 0 || y < 0 || x >= innerWidth || y >= innerHeight) return false;
        let hit = document.elementFromPoint(x, y);
        while (hit?.shadowRoot) {
          const inner = hit.shadowRoot.elementFromPoint(x, y);
          if (!inner || inner === hit) break;
          hit = inner;
        }
        return hit === node || node.contains(hit);
      }),
    )
    .toBe(true);
  return search;
}

for (const width of [1280, 940, 390]) {
  test(`HS2-CSP42A reveals reachable workspace search from the inspector at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    const rail = page.locator('#app-right-rail'),
      ticket = page.locator('[data-component="summary-row"][data-ticket-slug="HS2-QUEUE1"]').first(),
      compact = width < 1024;
    await test.step('Open the synthetic project and its ticket inspector', async () => {
      await mockSearchProject(page);
      await ticket.click({ timeout: 10_000 });
      if (!compact && (await rail.getAttribute('data-collapsed')) === 'true')
        await page.getByRole('button', { name: 'Show Ticket Inspector' }).click({ timeout: 10_000 });
      await expect(rail).toHaveAttribute('data-collapsed', 'false');
    });

    // A real modal must continue to suppress the global shortcut without dismissing its rail.
    const reader = page.locator('[data-reader-dialog][open]');
    await test.step('Preserve native reader shortcut suppression and close it', async () => {
      await rail.getByRole('button', { name: 'Open Ticket Reader' }).click({ timeout: 10_000 });
      await expect(reader).toBeVisible();
      await page.keyboard.press('ControlOrMeta+k');
      await expect(reader).toBeVisible();
      await expect(rail).toHaveAttribute('data-collapsed', 'false');
      await expect(page.getByRole('searchbox', { name: 'Search tickets' })).toHaveCount(0);
      await page.keyboard.press('Escape');
      // This locator selects only open native dialogs; a retained closed frame is also acceptable.
      await expect(reader).toHaveCount(0);
      await expect(page.locator('dialog:modal')).toHaveCount(0);
    });

    await page.keyboard.press('ControlOrMeta+k');
    await expect(rail).toHaveAttribute('data-collapsed', compact ? 'true' : 'false');
    const search = await expectReachableWorkspaceSearch(page);
    if (width === 390)
      await page.screenshot({ path: 'target/visual-captures/HS2-CSP42A-after-reader-390.png', animations: 'disabled' });
    await search.pressSequentially('badge');
    await expect(ticket).toBeVisible();
    await expect(page.locator('[data-ticket-slug="HS2-QUEUE2"]')).toHaveCount(0);

    // Selecting a result reopens the phone inspector; a second search intent preserves its query.
    await ticket.click({ timeout: 10_000 });
    await expect(rail).toHaveAttribute('data-collapsed', 'false');
    await page.keyboard.press('ControlOrMeta+k');
    await expect(rail).toHaveAttribute('data-collapsed', compact ? 'true' : 'false');
    await expect(await expectReachableWorkspaceSearch(page)).toHaveText('badge');
    await page.screenshot({ path: `target/visual-captures/HS2-CSP42A-search-${width}.png`, animations: 'disabled' });
  });
}

test('HS2-CSP42A compact search dismissal preserves the wide inspector preference through resize', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await mockSearchProject(page);
  const rail = page.locator('#app-right-rail'),
    ticket = page.locator('[data-component="summary-row"][data-ticket-slug="HS2-QUEUE1"]').first();
  await ticket.click({ timeout: 10_000 });
  if ((await rail.getAttribute('data-collapsed')) === 'true')
    await page.getByRole('button', { name: 'Show Ticket Inspector' }).click({ timeout: 10_000 });
  await expect(rail).toHaveAttribute('data-collapsed', 'false');
  for (const width of [940, 390]) {
    await page.setViewportSize({ width, height: 900 });
    await expect(rail).toHaveAttribute('data-presentation', 'overlay');
    await expect(rail).toHaveAttribute('data-collapsed', 'true');
    await ticket.click({ timeout: 10_000 });
    await expect(rail).toHaveAttribute('data-collapsed', 'false');
    await page.keyboard.press('ControlOrMeta+k');
    await expect(rail).toHaveAttribute('data-collapsed', 'true');
    await expectReachableWorkspaceSearch(page);
    await page.screenshot({
      path: `target/visual-captures/HS2-CSP42A-search-resize-${width}.png`,
      animations: 'disabled',
    });
  }
  await page.setViewportSize({ width: 1280, height: 900 });
  await expect(rail).toHaveAttribute('data-presentation', 'inline');
  await expect(rail).toHaveAttribute('data-collapsed', 'false');
  await page.keyboard.press('ControlOrMeta+k');
  await expectReachableWorkspaceSearch(page);
  await expect(rail).toHaveAttribute('data-collapsed', 'false');
  await page.screenshot({ path: 'target/visual-captures/HS2-CSP42A-search-returned-wide.png', animations: 'disabled' });
});

for (const width of [1280, 390]) {
  test(`searching from Trash searches and counts only Trash at ${width}px (HS2-S6TXSW)`, async ({ page }) => {
    await page.setViewportSize({ width, height: width > 600 ? 820 : 844 });
    const searches = await mockSearchProject(page);
    let navigation = await viewNavigation(page);
    await navigation.getByRole('button', { name: /^Trash/ }).click();
    await closeOverlaySidebar(page);
    await expect(page.locator('[data-ticket-slug="HS2-TRASH2"]').first()).toBeVisible();
    await page.getByRole('button', { name: 'Search tickets' }).click();
    await page.getByRole('searchbox', { name: 'Search tickets' }).fill('badge');
    await expect(page.locator('[data-ticket-slug="HS2-TRASH1"]').first()).toBeVisible();
    await expect(page.locator('[data-ticket-slug="HS2-TRASH2"]')).toHaveCount(0);
    await expect(page.locator('[data-ticket-slug="HS2-QUEUE1"]')).toHaveCount(0);
    navigation = await viewNavigation(page);
    // Only the Trash row carries the search icon and a search-relative count.
    await expect(viewCount(navigation, 'Trash')).toHaveAttribute('data-search-count', 'true');
    await expect(navigation.getByLabel('1 search results')).toBeVisible();
    await expect(navigation.locator('[data-search-count="true"]')).toHaveCount(1);
    await expect(navigation.locator('[data-lucide="search"]')).toHaveCount(1);
    await expect(viewCount(navigation, 'Queue')).toHaveText('3');
    await expect(viewCount(navigation, 'Backlog')).toHaveText('2');
    await expect(viewCount(navigation, 'Archive')).toHaveText('1');
    // The search never ran against the other views.
    expect([...new Set(searches)]).toEqual(['trash']);
    // Let the overlay sidebar finish sliding in before the after-capture.
    await expect(page.locator('#app-left-rail .kui-workbench__panel-content')).toHaveCSS('transform', 'none');
    await page.waitForTimeout(300);
    await page.screenshot({ path: `target/visual-captures/hs2-s6txsw-trash-search-${width}.png` });

    // Searching from Queue never counts Trash, which keeps its plain total.
    await navigation.getByRole('button', { name: /^Queue/ }).click();
    await closeOverlaySidebar(page);
    await expect(page.locator('[data-ticket-slug="HS2-QUEUE1"]').first()).toBeVisible();
    navigation = await viewNavigation(page);
    await expect(navigation.locator('[data-search-count="true"]')).toHaveCount(3);
    await expect(viewCount(navigation, 'Trash')).not.toHaveAttribute('data-search-count', 'true');
    await expect(viewCount(navigation, 'Trash')).toHaveText('2');
    expect(searches.slice(searches.lastIndexOf('trash') + 1).sort()).toEqual(['archive', 'backlog', 'queue']);
  });
}

const columnNames = (board: Locator) =>
  board
    .locator('[data-component="ticket-board-column"]')
    .evaluateAll((columns) => columns.map((column) => column.getAttribute('aria-label')));

for (const width of [1440, 1024]) {
  test(`terminal ticket rail searches All Tickets and returns to its view at ${width}px (HS2-5WXA6W)`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: 820 });
    const searches = await mockSearchProject(page);
    await page.getByRole('button', { name: 'Workspace Grid' }).click();
    const rail = page.locator('#app-right-rail'),
      toolbar = rail.getByRole('toolbar', { name: 'Search Scope' });
    await expect(toolbar).toHaveCount(0);
    await rail.getByRole('button', { name: 'Search tickets' }).click();
    await rail.getByRole('searchbox', { name: 'Search tickets' }).fill('badge');
    await expect(rail.locator('[data-ticket-slug="HS2-QUEUE1"]')).toBeVisible();
    await expect(rail.locator('[data-ticket-slug="HS2-BACK01"]')).toHaveCount(0);
    await expect(toolbar.getByRole('button', { name: 'Queue' })).toHaveAttribute('aria-pressed', 'true');
    await toolbar.getByRole('button', { name: 'All Tickets' }).click();
    await expect(toolbar.getByRole('button', { name: 'All Tickets' })).toHaveAttribute('aria-pressed', 'true');
    await expect(rail.locator('.terminal-ticket-rail__heading')).toContainText('All Tickets');
    for (const slug of ['HS2-QUEUE1', 'HS2-DONE01', 'HS2-BACK01', 'HS2-ARCH01'])
      await expect(rail.locator(`[data-ticket-slug="${slug}"]`)).toBeVisible();
    await expect(rail.locator('[data-ticket-slug="HS2-TRASH1"]')).toHaveCount(0);
    expect(searches).toContain('all');
    await expect(page.getByRole('toolbar', { name: 'Search Scope' })).toHaveCount(1);
    await rail.screenshot({ path: test.info().outputPath(`hs2-5wxa6w-rail-all-list-${width}.png`) });
    await page.screenshot({ path: test.info().outputPath(`hs2-5wxa6w-grid-all-list-${width}.png`) });
    await rail.getByRole('button', { name: /Columns View/ }).click();
    const board = rail.locator('[data-component="ticket-board"]');
    await expect
      .poll(() => columnNames(board))
      .toEqual([
        'Backlog Column',
        'Not Started Column',
        'Started Column',
        'Completed Column',
        'Verified Column',
        'Archive Column',
      ]);
    await expect(board.locator('[data-ticket-slug="HS2-BACK01"]')).toBeVisible();
    await rail.screenshot({ path: test.info().outputPath(`hs2-5wxa6w-rail-all-columns-${width}.png`) });
    await toolbar.getByRole('button', { name: 'Queue' }).click();
    await expect(rail.locator('.terminal-ticket-rail__heading')).toContainText('Queue');
    await expect(rail.locator('[data-ticket-slug="HS2-BACK01"]')).toHaveCount(0);
    await expect
      .poll(() => columnNames(board))
      .toEqual(['Not Started Column', 'Started Column', 'Completed Column', 'Verified Column']);
    await rail.getByRole('button', { name: /Notifications View/ }).click();
    await expect(toolbar).toHaveCount(0);
  });
}

for (const width of [1280, 390]) {
  test(`switches a search into All Tickets and back from the floating toolbar at ${width}px (HS2-4J6764)`, async ({
    page,
  }) => {
    // A long multi-step flow; leave headroom when several agents share the machine.
    test.setTimeout(120_000);
    await page.setViewportSize({ width, height: width > 600 ? 820 : 844 });
    const searches = await mockSearchProject(page);
    const toolbar = page.getByRole('toolbar', { name: 'Search Scope' }),
      toggle = toolbar.getByRole('button', { name: 'All Tickets' }),
      viewChoice = toolbar.getByRole('button', { name: 'Queue' }),
      title = page.locator('#workspace-page-title'),
      search = page.getByRole('searchbox', { name: 'Search tickets' }),
      // Phones name a sidebar view in the page header's view picker and All Tickets in its title.
      expectTitle = async (text: string) => {
        if (width > 600 || text === 'All Tickets') await expect(title).toHaveText(text);
        else {
          await expect(title).toHaveCount(0);
          await expect(page.locator('[name="mobile-view"]')).toContainText(text);
        }
      };
    // No search, no scope toolbar.
    await expect(toolbar).toHaveCount(0);
    await page.getByRole('button', { name: 'Search tickets' }).click();
    await search.fill('badge');
    await expect(page.locator('[data-ticket-slug="HS2-QUEUE1"]').first()).toBeVisible();
    await expect(page.locator('[data-ticket-slug="HS2-BACK01"]')).toHaveCount(0);
    await expect(toggle).toHaveAttribute('aria-pressed', 'false');
    await expect(viewChoice).toHaveAttribute('aria-pressed', 'true');
    // The toolbar floats centered at the bottom of the ticket work area.
    const area = (await page.getByLabel('Ticket work area').boundingBox())!,
      bar = (await toolbar.boundingBox())!;
    expect(Math.abs(bar.x + bar.width / 2 - (area.x + area.width / 2))).toBeLessThan(2);
    expect(area.y + area.height - (bar.y + bar.height)).toBeLessThan(40);
    expect(area.y + area.height - (bar.y + bar.height)).toBeGreaterThanOrEqual(0);
    await toolbar.screenshot({ path: `target/visual-captures/hs2-4j6764-scope-toolbar-view-${width}.png` });

    // All Tickets searches every collection except Trash.
    searches.length = 0;
    await toggle.click();
    await expect(toggle).toHaveAttribute('aria-pressed', 'true');
    await expectTitle('All Tickets');
    for (const slug of ['HS2-QUEUE1', 'HS2-DONE01', 'HS2-BACK01', 'HS2-ARCH01'])
      await expect(page.locator(`[data-ticket-slug="${slug}"]`).first()).toBeVisible();
    await expect(page.locator('[data-ticket-slug="HS2-TRASH1"]')).toHaveCount(0);
    expect(searches[0]).toBe('all');
    expect(searches).not.toContain('trash');
    await page.screenshot({ path: `target/visual-captures/hs2-4j6764-all-tickets-list-${width}.png` });
    await toolbar.screenshot({ path: `target/visual-captures/hs2-4j6764-scope-toolbar-all-${width}.png` });
    // The sidebar shows each view's share of the All Tickets search and selects none of them.
    const navigation = await viewNavigation(page);
    await expect(navigation.locator('[data-search-count="true"]')).toHaveCount(3);
    await expect(viewCount(navigation, 'Queue')).toContainText('2');
    await expect(viewCount(navigation, 'Backlog')).toContainText('1');
    await expect(viewCount(navigation, 'Archive')).toContainText('1');
    await expect(viewCount(navigation, 'Trash')).not.toHaveAttribute('data-search-count', 'true');
    await expect(navigation.locator('[aria-current]')).toHaveCount(0);
    await expect(page.locator('#app-left-rail .kui-workbench__panel-content')).toHaveCSS('transform', 'none');
    await page.waitForTimeout(300);
    await page.screenshot({ path: `target/visual-captures/hs2-4j6764-all-tickets-sidebar-${width}.png` });
    await closeOverlaySidebar(page);

    // Columns: Backlog, the lifecycle columns, then Archive; never Trash.
    const columnsView = page.getByRole('button', { name: 'Columns View' });
    if (await columnsView.isVisible()) await columnsView.click();
    else {
      await page.getByRole('button', { name: 'More Workspace Controls' }).click();
      await page.getByRole('menuitemcheckbox', { name: 'Show Columns View' }).click();
    }
    const board = page.locator('[data-component="ticket-board"]');
    await expect
      .poll(() => columnNames(board))
      .toEqual([
        'Backlog Column',
        'Not Started Column',
        'Started Column',
        'Completed Column',
        'Verified Column',
        'Archive Column',
      ]);
    await expect(board.locator('[data-ticket-slug="HS2-BACK01"]')).toHaveCount(1);
    await page.waitForTimeout(300);
    await page.screenshot({ path: `target/visual-captures/hs2-4j6764-all-tickets-columns-${width}.png` });
    // The toolbar paints above the board columns, not under them.
    expect(
      await toolbar.evaluate((node) => {
        const box = node.getBoundingClientRect(),
          hit = document.elementFromPoint(box.x + box.width / 2, box.y + box.height / 2);
        return node.contains(hit);
      }),
    ).toBe(true);

    // Choosing the view again returns the search to the view it started from.
    await viewChoice.click();
    await expect(toggle).toHaveAttribute('aria-pressed', 'false');
    await expectTitle('Queue');
    await expect
      .poll(() => columnNames(board))
      .toEqual(['Not Started Column', 'Started Column', 'Completed Column', 'Verified Column']);
    await expect(page.locator('[data-ticket-slug="HS2-BACK01"]')).toHaveCount(0);

    // The mode lasts only while the search does.
    await toggle.click();
    await expectTitle('All Tickets');
    await search.fill('');
    await expect(toolbar).toHaveCount(0);
    await expectTitle('Queue');
    await search.fill('badge');
    await expect(toggle).toHaveAttribute('aria-pressed', 'false');
    await expectTitle('Queue');

    // Choosing a sidebar view leaves All Tickets for that view; Trash never offers it.
    await toggle.click();
    await expectTitle('All Tickets');
    await (await viewNavigation(page)).getByRole('button', { name: /^Backlog/ }).click();
    await closeOverlaySidebar(page);
    await expectTitle('Backlog');
    await expect(toggle).toHaveAttribute('aria-pressed', 'false');
    await (await viewNavigation(page)).getByRole('button', { name: /^Trash/ }).click();
    await closeOverlaySidebar(page);
    await expect(page.locator('[data-ticket-slug="HS2-TRASH1"]').first()).toBeVisible();
    await expect(toolbar).toHaveCount(0);
  });
}

/** Whether the workspace title shows its whole text rather than an ellipsis. */
const titleFits = (page: Page) =>
  page.locator('#workspace-page-title').evaluate((title) => {
    const text = title.querySelector<HTMLElement>('.kui-toolbar-text__text') ?? (title as HTMLElement);
    return text.scrollWidth <= text.clientWidth;
  });

test('keeps the workspace title whole when a search changes it to a longer title at its toolbar-fit boundary (HS2-2RD2M9)', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 820 });
  await mockSearchProject(page, [{ id: 'long-view', name: 'Release blockers for the spring launch', query: 'badge' }]);
  const title = page.locator('#workspace-page-title'),
    utility = page.getByRole('group', { name: 'View Actions' }),
    more = page.getByRole('button', { name: 'More Workspace Controls' });
  const expectOverflowActions = async () => {
    await more.click();
    await expect(page.getByRole('menuitem', { name: /^Toggle Up Next/ })).toBeVisible();
    await expect(page.getByRole('menuitem', { name: 'Show Selected Ticket Actions…', exact: true })).toBeVisible();
    await page.keyboard.press('Escape');
  };
  const resize = async (width: number) => {
    await page.setViewportSize({ width, height: 820 });
    // Let the ResizeObserver apply the measured fit before sampling visibility.
    await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  };
  await page.getByRole('button', { name: 'Search tickets' }).click();
  await page.getByRole('searchbox', { name: 'Search tickets' }).fill('badge');
  await expect(title).toHaveText('Queue');
  await expect.poll(() => titleFits(page)).toBe(true);
  // HS2-P9A21R: five workspace modes already overflow actions at 1280px. They remain reachable.
  await expect(utility).toBeHidden();
  await expect(more).toBeVisible();
  await expectOverflowActions();
  // Find the rendered short-title boundary within our representative desktop widths, rather
  // than pinning a pixel threshold that changes whenever a legitimate toolbar control is added.
  let overflowing = 1280,
    fitting = 1440;
  await resize(fitting);
  await expect(utility).toBeVisible();
  while (fitting - overflowing > 1) {
    const width = Math.floor((overflowing + fitting) / 2);
    await resize(width);
    if (await utility.isVisible()) fitting = width;
    else overflowing = width;
  }
  await resize(fitting);
  await expect(utility).toBeVisible();
  await expect.poll(() => titleFits(page)).toBe(true);
  // All Tickets changes only the title's text; the header remeasures and yields actions to More.
  await page.getByRole('toolbar', { name: 'Search Scope' }).getByRole('button', { name: 'All Tickets' }).click();
  await expect(title).toHaveText('All Tickets');
  await expect.poll(() => titleFits(page)).toBe(true);
  await expect(utility).toBeHidden();
  await expect(more).toBeVisible();
  await expectOverflowActions();
  await page.screenshot({ path: 'target/visual-captures/hs2-p9a21r-all-tickets-search-fit-boundary.png' });
  // Back to the shorter title, every action returns.
  await page.getByRole('toolbar', { name: 'Search Scope' }).getByRole('button', { name: 'Queue' }).click();
  await expect(title).toHaveText('Queue');
  await expect(utility).toBeVisible();
  await expect.poll(() => titleFits(page)).toBe(true);
  // A long saved-view name stays whole too.
  await page
    .getByRole('navigation', { name: 'Ticket Views' })
    .getByRole('button', { name: /^Release blockers/ })
    .click();
  await expect(title).toHaveText('Release blockers for the spring launch');
  await expect.poll(() => titleFits(page)).toBe(true);
  await expectOverflowActions();
  await expect(page.getByText('Searching tickets')).toHaveCount(0);
  await resize(1280);
  await expect.poll(() => titleFits(page)).toBe(true);
  await page.screenshot({ path: 'target/visual-captures/hs2-2rd2m9-long-view-search-1280.png' });
});
