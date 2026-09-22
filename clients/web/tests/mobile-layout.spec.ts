import { expect, test } from '@playwright/test';

// Mobile single-column layout with overlay sidebars (HS2-ZK51WP): below the desktop size floor the
// project sidebar and ticket inspector overlay the single main column, only one is open at a time,
// and a click-away scrim dismisses whichever is open.

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
const project = (id: string, root: string) => ({
  id,
  root,
  name: root.split('/').at(-1),
  stores: [`${root}.hs2`],
  apiPath: `/__hotsheet/project-api/${id}`,
});
const ticket = (slug: string, status: string) => ({
  connection_id: 'git-local',
  native_id: slug,
  qualified_id: `git-local:${slug}`,
  id: slug,
  slug,
  title: slug,
  status,
  up_next: false,
  feedback_needed: false,
  tags: [],
  blocked_by: [],
  claim_count: 0,
});

async function openDemoProject(page: import('@playwright/test').Page, withTerminal = false, ticketCount = 1) {
  await page.route('**/*', async (route) => {
    const request = route.request(),
      url = new URL(request.url()),
      path = url.pathname;
    if (path === '/__hotsheet/projects/open')
      return route.fulfill({ status: 201, json: project('demo-checkout', request.postDataJSON().root as string) });
    if (path === '/__hotsheet/folders/choose') return route.fulfill({ json: { path: '/work/demo' } });
    if (path.endsWith('/providers'))
      return route.fulfill({
        json: [
          {
            connection_id: 'git-local',
            provider: 'git',
            display_name: 'Hot Sheet git',
            locator: '/tickets',
            default: true,
            capabilities,
          },
        ],
      });
    // Flattened ticket + store, matching GET /checkouts/{ref}/tickets/{id} (the client wraps it itself).
    if (/\/tickets\/HS2-M1$/.test(path))
      return route.fulfill({
        json: {
          store: 'git-local',
          ...ticket('HS2-M1', 'started'),
          category: 'bug',
          priority: 'default',
          details: '',
          blocked_reason: null,
          notes: [],
          attachments: [],
          created_at: '2026-09-01T00:00:00Z',
          updated_at: '2026-09-14T00:00:00Z',
          concurrency_token: 't1',
        },
      });
    if (path.endsWith('/tickets'))
      return route.fulfill({
        json: {
          items: Array.from({ length: ticketCount }, (_, index) => ticket(`HS2-M${index + 1}`, 'started')),
          counts: {
            total: ticketCount,
            queued: ticketCount,
            backlog: 0,
            archive: 0,
            open: ticketCount,
            up_next: 0,
            active: 0,
            started: ticketCount,
            completed_today: 0,
            completion_trend: [0, 0, 0, 0, 0, 0, 0],
          },
        },
      });
    if (path.endsWith('/repository/status'))
      return route.fulfill({
        json: { branch: 'main', ahead: 0, behind: 0, staged: 0, unstaged: 0, untracked: 0, conflicted: 0, clean: true },
      });
    if (path.endsWith('/ws/poll'))
      return route.fulfill({
        json: { cursor: Number(url.searchParams.get('since') ?? 0), events: [], overflow: false },
      });
    if (path.endsWith('/terminals'))
      return route.fulfill({
        json: withTerminal ? [{ id: 'codex-main', alive: true, busy: false, cwd: '/work/demo' }] : [],
      });
    if (
      path.endsWith('/connections') ||
      path.endsWith('/commands') ||
      path.endsWith('/command-runs') ||
      path.endsWith('/views') ||
      path.endsWith('/corrupt-tickets')
    )
      return route.fulfill({ json: [] });
    return route.continue();
  });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('textbox', { name: /^Project folder/ }).fill('/work/demo');
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
}

test('mobile floating controls stay inside the dynamic viewport and safe area (HS2-43N9ZB)', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await openDemoProject(page, true);
  await page.evaluate(() => {
    document.documentElement.style.setProperty('--hotsheet-safe-area-bottom', '48px');
  });
  const restore = page.getByRole('button', { name: 'Show terminal drawer' }),
    restoreToolbar = page.locator('.app-shell__terminal-drawer-restore');
  await expect(restore).toBeVisible();
  const rootGeometry = await page.evaluate(() => ({
    innerHeight,
    html: document.documentElement.getBoundingClientRect().height,
    body: document.body.getBoundingClientRect().height,
    app: document.querySelector('#app')!.getBoundingClientRect().height,
    shell: document.querySelector('[data-component="app-shell"]')!.getBoundingClientRect().height,
  }));
  expect(rootGeometry).toEqual({ innerHeight: 844, html: 844, body: 844, app: 844, shell: 844 });
  const restoreBottom = await restoreToolbar.evaluate((node) => innerHeight - node.getBoundingClientRect().bottom);
  expect(restoreBottom).toBeCloseTo(64, 0);
  await page.screenshot({ path: '/private/tmp/hs2-43n9zb-mobile-drawer-restore.png', fullPage: true });
  await page.getByRole('button', { name: 'Workspace grid' }).click();
  const zoom = page.getByRole('toolbar', { name: 'Workspace tile zoom' }),
    zoomToolbar = page.locator('.terminal-dashboard__zoom');
  await expect(zoom).toBeVisible();
  const zoomBottom = await zoomToolbar.evaluate((node) => innerHeight - node.getBoundingClientRect().bottom);
  expect(zoomBottom).toBeCloseTo(64, 0);
  await page.screenshot({ path: '/private/tmp/hs2-43n9zb-mobile-floating-controls.png', fullPage: true });
});

test('mobile viewport uses a single-column layout with overlay sidebars, one at a time (HS2-ZK51WP)', async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await openDemoProject(page);
  const shell = page.locator('[data-component="app-shell"]');
  await expect(shell).toHaveAttribute('data-mobile', 'true');
  const sidebar = page.locator('.kui-resizable-region[data-region-id="app-sidebar"]');
  const inspector = page.locator('.kui-resizable-region[data-region-id="app-inspector"]');
  const scrim = page.locator('.app-shell__scrim');
  // Single column: both overlays start closed and the scrim is absent.
  await expect(sidebar).toHaveAttribute('data-collapsed', 'true');
  await expect(inspector).toHaveAttribute('data-collapsed', 'true');
  await expect(scrim).toHaveCount(0);

  // Open the project sidebar as an overlay.
  await page.getByRole('button', { name: 'Show project sidebar' }).click();
  await expect(sidebar).toHaveAttribute('data-collapsed', 'false');
  await expect(scrim).toBeVisible();
  // The sidebar overlays the main column rather than sitting beside it.
  await expect(sidebar).toHaveCSS('position', 'absolute');

  await page.screenshot({ path: '/private/tmp/hs2-zk51wp-mobile-sidebar-overlay.png', fullPage: true });

  // Click-away scrim dismisses the open sidebar and returns to a single column. (The scrim covers
  // the toolbar, so switching overlays is dismiss-then-open — normal mobile drawer behavior.) Click
  // the scrim strip on the side the left sidebar does not cover.
  await scrim.click({ position: { x: 370, y: 400 } });
  await expect(sidebar).toHaveAttribute('data-collapsed', 'true');
  await expect(scrim).toHaveCount(0);

  // The inspector opens as its own overlay while the sidebar stays closed (one at a time).
  await page.getByRole('button', { name: 'Show ticket inspector' }).click();
  await expect(inspector).toHaveAttribute('data-collapsed', 'false');
  await expect(sidebar).toHaveAttribute('data-collapsed', 'true');
  await expect(scrim).toBeVisible();
  await page.screenshot({ path: '/private/tmp/hs2-zk51wp-mobile-inspector-overlay.png', fullPage: true });

  // Dismiss via the scrim strip the right inspector does not cover.
  await scrim.click({ position: { x: 10, y: 400 } });
  await expect(inspector).toHaveAttribute('data-collapsed', 'true');
  await expect(scrim).toHaveCount(0);
  await page.screenshot({ path: '/private/tmp/hs2-zk51wp-mobile-single-column.png', fullPage: true });
});

test('mobile keyboard shortcuts toggle mutually exclusive sidebar overlays without changing desktop preferences (HS2-KN79XP)', async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await openDemoProject(page);
  const modifier = await page.evaluate(() =>
    /macintosh|mac os|iphone|ipad|ipod/i.test(navigator.userAgent) ? 'Meta' : 'Control',
  );
  const sidebar = page.locator('.kui-resizable-region[data-region-id="app-sidebar"]'),
    inspector = page.locator('.kui-resizable-region[data-region-id="app-inspector"]'),
    scrim = page.locator('.app-shell__scrim');
  await page.locator('[data-ticket-slug="HS2-M1"]').click();
  await expect(inspector).toHaveAttribute('data-collapsed', 'false');
  await scrim.click({ position: { x: 10, y: 400 } });
  await expect(inspector).toHaveAttribute('data-collapsed', 'true');
  await page.keyboard.press(`${modifier}+b`);
  await expect(sidebar).toHaveAttribute('data-collapsed', 'false');
  await expect(inspector).toHaveAttribute('data-collapsed', 'true');
  await expect(scrim).toBeVisible();
  await page.keyboard.press(`${modifier}+Alt+Shift+b`);
  await expect(sidebar).toHaveAttribute('data-collapsed', 'true');
  await expect(inspector).toHaveAttribute('data-collapsed', 'false');
  await expect(scrim).toBeVisible();
  await page.screenshot({ path: '/private/tmp/hs2-kn79xp-mobile-keyboard-overlays.png', fullPage: true });
  await page.keyboard.press(`${modifier}+Alt+Shift+b`);
  await expect(inspector).toHaveAttribute('data-collapsed', 'true');
  await expect(scrim).toHaveCount(0);
  await page.setViewportSize({ width: 1280, height: 800 });
  await expect(sidebar).toHaveAttribute('data-collapsed', 'false');
  await expect(inspector).toHaveAttribute('data-collapsed', 'false');
});

test('mobile forces list view and hides the columns toggle, restoring board view on desktop (HS2-1XCHZT)', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await openDemoProject(page);
  await expect(page.locator('[data-ticket-slug="HS2-M1"]')).toBeVisible();
  // Desktop: the columns/board toggle is available and switches to a board.
  await page.getByRole('button', { name: 'Columns view' }).click();
  await expect(page.locator('[data-component="ticket-board"]')).toBeVisible();
  await expect(page.locator('[data-component="ticket-list"]')).toHaveCount(0);

  // Shrinking below the floor forces the list view and removes the columns toggle entirely.
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(page.getByRole('button', { name: 'Columns view' })).toHaveCount(0);
  await expect(page.locator('[data-component="ticket-list"]')).toBeVisible();
  await expect(page.locator('[data-component="ticket-board"]')).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'List view' })).toHaveAttribute('aria-pressed', 'true');

  // Growing back restores the desktop board preference (it was never overwritten).
  await page.setViewportSize({ width: 1280, height: 800 });
  await expect(page.locator('[data-component="ticket-board"]')).toBeVisible();
  await expect(page.getByRole('button', { name: 'Columns view' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Columns view' })).toHaveAttribute('aria-pressed', 'true');
  await expect(page.getByRole('button', { name: 'List view' })).toHaveAttribute('aria-pressed', 'false');
  await page.getByRole('button', { name: 'List view' }).click();
  await expect(page.locator('[data-component="ticket-list"]')).toBeVisible();
  await expect(page.getByRole('button', { name: 'List view' })).toHaveAttribute('aria-pressed', 'true');
});

test('tapping a ticket auto-opens the inspector overlay, and tap-away returns to the list (HS2-N7RPFP)', async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await openDemoProject(page);
  const inspector = page.locator('.kui-resizable-region[data-region-id="app-inspector"]');
  const scrim = page.locator('.app-shell__scrim');
  await expect(page.locator('[data-ticket-slug="HS2-M1"]')).toBeVisible();
  // Inspector starts closed on mobile.
  await expect(inspector).toHaveAttribute('data-collapsed', 'true');

  // Tapping the ticket row auto-opens the inspector overlay and shows the ticket.
  await page.locator('[data-action="select-ticket-row"][data-ticket-slug="HS2-M1"]').click();
  await expect(inspector).toHaveAttribute('data-collapsed', 'false');
  await expect(scrim).toBeVisible();
  await expect(page.locator('[data-component="ticket-inspector"]')).toBeVisible();

  // Tap-away on the scrim returns to the list; the selection persists so tapping reopens it.
  await scrim.click({ position: { x: 10, y: 400 } });
  await expect(inspector).toHaveAttribute('data-collapsed', 'true');
  await expect(scrim).toHaveCount(0);
  await page.locator('[data-action="select-ticket-row"][data-ticket-slug="HS2-M1"]').click();
  await expect(inspector).toHaveAttribute('data-collapsed', 'false');
});

test('mobile replaces the project tabs and view title with select controls (HS2-4C5RM7)', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.route('**/*', async (route) => {
    const request = route.request(),
      url = new URL(request.url()),
      path = url.pathname,
      other = path.includes('other-checkout');
    if (path === '/__hotsheet/projects/open') {
      const root = request.postDataJSON().root as string;
      return route.fulfill({
        status: 201,
        json: root === '/work/other' ? project('other-checkout', root) : project('demo-checkout', root),
      });
    }
    // Only the mobile Add-project button reaches the folder chooser here (the first project opens via the dialog form).
    if (path === '/__hotsheet/folders/choose') return route.fulfill({ json: { path: '/work/other' } });
    if (path.endsWith('/providers'))
      return route.fulfill({
        json: [
          {
            connection_id: 'git-local',
            provider: 'git',
            display_name: 'Hot Sheet git',
            locator: '/tickets',
            default: true,
            capabilities,
          },
        ],
      });
    if (path.endsWith('/tickets'))
      return route.fulfill({
        json: {
          items: other ? [ticket('HS2-OTHER1', 'started')] : [ticket('HS2-M1', 'started')],
          counts: {
            total: 1,
            queued: 1,
            backlog: 0,
            archive: 0,
            open: 1,
            up_next: 0,
            active: 0,
            started: 1,
            completed_today: 0,
            completion_trend: [0, 0, 0, 0, 0, 0, 0],
          },
        },
      });
    if (path.endsWith('/repository/status'))
      return route.fulfill({
        json: { branch: 'main', ahead: 0, behind: 0, staged: 0, unstaged: 0, untracked: 0, conflicted: 0, clean: true },
      });
    if (path.endsWith('/ws/poll'))
      return route.fulfill({
        json: { cursor: Number(url.searchParams.get('since') ?? 0), events: [], overflow: false },
      });
    if (
      path.endsWith('/terminals') ||
      path.endsWith('/connections') ||
      path.endsWith('/commands') ||
      path.endsWith('/command-runs') ||
      path.endsWith('/views') ||
      path.endsWith('/corrupt-tickets')
    )
      return route.fulfill({ json: [] });
    return route.continue();
  });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await expect(page.locator('[data-ticket-slug="HS2-M1"]')).toBeVisible();

  // The horizontal project tab strip is replaced by a project Select; no project tabs render.
  const projectSelect = page.locator('wa-select[name="mobile-project"]');
  await expect(projectSelect).toBeVisible();
  await expect(projectSelect).toHaveAttribute('value', 'demo-checkout');
  await expect(page.locator('[data-tab-kind="project"]')).toHaveCount(0);
  await expect(page.locator('.project-tab-bar--mobile')).toBeVisible();

  // The page-header view title is replaced by a view Select that switches ticket views.
  const viewSelect = page.locator('wa-select[name="mobile-view"]');
  await expect(viewSelect).toBeVisible();
  await expect(viewSelect).toHaveAttribute('value', 'all');
  await viewSelect.click();
  await viewSelect.locator('wa-option[value="backlog"]').click();
  await expect(viewSelect).toHaveJSProperty('value', 'backlog');
  // Programmatic state → live control (bidirectional binding).
  await viewSelect.evaluate((node: HTMLElement & { value: string }) => {
    node.value = 'all';
    node.dispatchEvent(new Event('change', { bubbles: true }));
  });
  await expect(viewSelect).toHaveJSProperty('value', 'all');

  // Adding a project opens and activates it; the project Select then switches the active project.
  await page.locator('.project-tab-bar--mobile').getByRole('button', { name: 'Add project' }).click();
  await expect(projectSelect).toHaveAttribute('value', 'other-checkout');
  await expect(page.locator('[data-ticket-slug="HS2-OTHER1"]')).toBeVisible();
  await projectSelect.click();
  await projectSelect.locator('wa-option[value="demo-checkout"]').click();
  await expect(projectSelect).toHaveJSProperty('value', 'demo-checkout');
  await expect(page.locator('[data-ticket-slug="HS2-M1"]')).toBeVisible();
});

test('mobile toolbar drops the project name, uses borderless content-fit selects, and hides the segmented control while searching (HS2-0SARDD)', async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await openDemoProject(page);
  await expect(page.locator('[data-ticket-slug="HS2-M1"]')).toBeVisible();
  // 1. The project name is gone from the main toolbar (the mobile project Select carries it instead).
  await expect(page.locator('[data-component="workspace-identity"]')).toHaveCount(0);
  // 2 & 3. The project and view selects are borderless (their combobox part has no border).
  const comboBorder = (name: string) =>
    page.locator(`wa-select[name="${name}"]`).evaluate((node) => {
      const part = (node as HTMLElement).shadowRoot?.querySelector('[part~="combobox"]');
      return part ? getComputedStyle(part).borderTopWidth : 'no-part';
    });
  expect(await comboBorder('mobile-project')).toBe('0px');
  expect(await comboBorder('mobile-view')).toBe('0px');
  // ...and sized to the selected label rather than stretching to fill the bar.
  const [selectBox, barBox] = await Promise.all([
    page.locator('wa-select[name="mobile-project"]').boundingBox(),
    page.locator('.project-tab-bar--mobile').boundingBox(),
  ]);
  expect(selectBox!.width).toBeLessThan(barBox!.width * 0.5);
  await page.screenshot({ path: '/private/tmp/claude/hs2-0sardd-mobile-toolbar.png', fullPage: true });
  // 4. Opening search hides the view-mode segmented control to give the field more space.
  const switcher = page.locator('.view-mode-switcher');
  await expect(switcher).toBeVisible();
  await page.getByRole('button', { name: 'Search tickets' }).click();
  await expect(page.getByLabel('Search tickets')).toBeVisible();
  await expect(switcher).toBeHidden();
  await page.screenshot({ path: '/private/tmp/claude/hs2-0sardd-mobile-search.png', fullPage: true });
});

test('resizing from mobile back to desktop restores the side-by-side layout (HS2-ZK51WP)', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await openDemoProject(page);
  const shell = page.locator('[data-component="app-shell"]');
  const sidebar = page.locator('.kui-resizable-region[data-region-id="app-sidebar"]');
  await page.getByRole('button', { name: 'Show project sidebar' }).click();
  await expect(sidebar).toHaveAttribute('data-collapsed', 'false');
  // Growing past the breakpoint drops mobile mode and its ephemeral open state.
  await page.setViewportSize({ width: 1280, height: 800 });
  await expect(shell).toHaveAttribute('data-mobile', 'false');
  await expect(sidebar).not.toHaveCSS('position', 'absolute');
  // Desktop default has the sidebar visible in-flow.
  await expect(sidebar).toHaveAttribute('data-collapsed', 'false');
});

test('keeps reopened inspector content within the viewport across desktop and mobile transitions (HS2-5JKNGS)', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 720 });
  await openDemoProject(page);
  await page.locator('[data-ticket-slug="HS2-M1"]').click();
  const shell = page.locator('[data-component="app-shell"]'),
    inspector = page.locator('.kui-resizable-region[data-region-id="app-inspector"]'),
    content = inspector.locator('[data-component="ticket-inspector"]');
  await expect(inspector).toHaveAttribute('data-collapsed', 'false');
  // Crossing into mobile intentionally closes desktop panels. Reopen through the public
  // control before measuring: a still-present closing panel is not an open overlay.
  for (const width of [940, 390, 1023, 1024, 1280]) {
    await page.setViewportSize({ width, height: 844 });
    await expect(shell).toHaveAttribute('data-mobile', String(width < 1024));
    if (width < 1024 && (await inspector.getAttribute('data-collapsed')) === 'true')
      await page.getByRole('button', { name: 'Show ticket inspector' }).click();
    await expect(inspector).toHaveAttribute('data-collapsed', 'false');
    await expect
      .poll(() =>
        content.evaluate((node) => {
          const rect = node.getBoundingClientRect();
          return rect.left >= -1 && rect.right <= innerWidth + 1 && rect.width > 250;
        }),
      )
      .toBe(true);
    const notes = content.locator('[data-component="ticket-notes"]');
    await notes.scrollIntoViewIfNeeded();
    await expect(notes.getByRole('button', { name: 'Add note', exact: true }).last()).toBeInViewport();
    await expect(notes.locator('.ticket-notes__empty')).toBeInViewport();
    if (width === 940 || width === 390)
      await page.screenshot({ path: `/private/tmp/hs2-5jkngs-inspector-${width}-settled.png`, animations: 'disabled' });
    if (width < 1024) {
      await content.getByRole('button', { name: 'Hide inspector', exact: true }).click();
      await expect(inspector).toHaveAttribute('data-collapsed', 'true');
      await expect(page.locator('.app-shell__scrim')).toHaveCount(0);
      await page.getByRole('button', { name: 'Show ticket inspector' }).click();
      await expect(inspector).toHaveAttribute('data-collapsed', 'false');
      await expect
        .poll(() => content.evaluate((node) => node.getBoundingClientRect().right <= innerWidth + 1))
        .toBe(true);
    }
  }
  await content
    .locator('[data-component="ticket-notes"]')
    .screenshot({ path: '/private/tmp/hs2-5jkngs-inspector-desktop-restored.png', animations: 'disabled' });
});

for (const initialWidth of [390, 1280]) {
  test(`keeps the mobile shell stationary through search focus, clear, typing and resize from ${initialWidth}px (HS2-JBTPNR)`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: initialWidth, height: 844 });
    await openDemoProject(page);
    const shell = page.locator('[data-component="app-shell"]'),
      editor = page.getByRole('searchbox', { name: 'Search tickets' }),
      field = page.locator('[data-token-search-id="workspace-search"]'),
      group = page.locator('.workspace-header__search-group').filter({ has: field });
    const expectContained = async (mobile: boolean) => {
      await expect.poll(() => shell.evaluate((node) => node.scrollLeft)).toBe(0);
      await expect
        .poll(() =>
          editor.evaluate((node) => {
            const rect = node.getBoundingClientRect();
            return rect.left >= 0 && rect.right <= innerWidth;
          }),
        )
        .toBe(true);
      if (mobile) {
        const [shellBox, mainBox] = await Promise.all([
          shell.boundingBox(),
          page.locator('.app-shell__main').boundingBox(),
        ]);
        expect(mainBox!.x).toBeCloseTo(shellBox!.x, 0);
        expect(mainBox!.width).toBeCloseTo(shellBox!.width, 0);
      }
    };
    for (const width of [initialWidth, 390, 1280]) {
      await page.setViewportSize({ width, height: 844 });
      await expect(shell).toHaveAttribute('data-mobile', String(width < 1024));
      await page.getByRole('button', { name: 'Search tickets', exact: true }).click();
      await expect(editor).toBeFocused();
      await expectContained(width < 1024);
      for (let attempt = 0; attempt < 2; attempt += 1) {
        await editor.fill('has:attachment ');
        await page.getByRole('button', { name: 'Clear search', exact: true }).click();
        await page.keyboard.type('continued');
        await expect(editor).toBeFocused();
        await expect(editor).toHaveText('continued');
        await expectContained(width < 1024);
      }
      await expect(page.getByText('Searching tickets', { exact: true })).toHaveCount(0);
      await page.screenshot({
        path: `/private/tmp/hs2-jbtpnr-search-${initialWidth}-to-${width}.png`,
        animations: 'disabled',
      });
      await page.getByRole('button', { name: 'Clear search', exact: true }).click();
      await page.getByRole('button', { name: 'Add project', exact: true }).focus();
      await expect(group).toHaveAttribute('data-expanded', 'false');
    }
  });
}

test('preserves child scrolling and usable mobile overlays after search focus (HS2-JBTPNR)', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 600 });
  await openDemoProject(page, false, 30);
  const shell = page.locator('[data-component="app-shell"]'),
    workspace = page.locator('.app-shell__workspace'),
    scrim = page.locator('.app-shell__scrim');
  await page.getByRole('button', { name: 'Search tickets', exact: true }).click();
  await page.getByRole('searchbox', { name: 'Search tickets' }).fill('continued');
  await page.getByRole('button', { name: 'Clear search', exact: true }).click();
  await page.getByRole('button', { name: 'Add project', exact: true }).focus();
  await workspace.hover();
  await page.mouse.wheel(0, 600);
  await expect.poll(() => workspace.evaluate((node) => node.scrollTop)).toBeGreaterThan(100);
  await expect.poll(() => shell.evaluate((node) => node.scrollLeft)).toBe(0);
  await page.getByRole('button', { name: 'Show project sidebar' }).click();
  const sidebar = page.locator('.project-sidebar');
  await expect
    .poll(() =>
      sidebar.evaluate((node) => {
        const rect = node.getBoundingClientRect();
        return rect.left >= -1 && rect.right <= innerWidth;
      }),
    )
    .toBe(true);
  await expect(sidebar.getByRole('button', { name: /^Queue / })).toBeInViewport();
  await page.screenshot({ path: '/private/tmp/hs2-jbtpnr-sidebar-mobile.png', animations: 'disabled' });
  await scrim.click({ position: { x: 380, y: 300 } });
  await workspace.hover();
  await page.mouse.wheel(0, -3000);
  await page.locator('[data-ticket-slug="HS2-M1"]').click();
  const inspector = page.locator('[data-component="ticket-inspector"]'),
    body = inspector.locator('.ticket-inspector__content');
  await expect
    .poll(() =>
      inspector.evaluate((node) => {
        const rect = node.getBoundingClientRect();
        return rect.left >= -1 && rect.right <= innerWidth + 1;
      }),
    )
    .toBe(true);
  await body.hover();
  await page.mouse.wheel(0, 600);
  await expect.poll(() => body.evaluate((node) => node.scrollTop)).toBeGreaterThan(0);
  await expect(inspector.getByRole('button', { name: 'Add note', exact: true }).last()).toBeInViewport();
  await page.screenshot({ path: '/private/tmp/hs2-jbtpnr-inspector-mobile.png', animations: 'disabled' });
  await inspector.getByRole('button', { name: 'Hide inspector', exact: true }).click();
  await expect(scrim).toHaveCount(0);
  await expect.poll(() => shell.evaluate((node) => node.scrollLeft)).toBe(0);
});
