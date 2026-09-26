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
    // ResizableRegion owns its restore control and consumes Kerf's component-safe-area contract.
    document.documentElement.style.setProperty(['--kui', 'safe-area-block-end'].join('-'), '48px');
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

test('gives a focused mobile terminal the visual viewport until explicit exit (HS2-GMTQZM)', async ({
  page,
}, testInfo) => {
  await page.addInitScript(() => {
    const viewport = Object.assign(new EventTarget(), {
      offsetLeft: 0,
      offsetTop: 0,
      width: 390,
      height: 844,
    });
    Object.defineProperty(window, 'visualViewport', { configurable: true, value: viewport });
    Object.defineProperty(window, '__setTerminalTestViewport', {
      configurable: true,
      value: (next: { left: number; top: number; width: number; height: number }) => {
        viewport.offsetLeft = next.left;
        viewport.offsetTop = next.top;
        viewport.width = next.width;
        viewport.height = next.height;
        viewport.dispatchEvent(new Event('resize'));
      },
    });
  });
  await page.setViewportSize({ width: 390, height: 844 });
  await openDemoProject(page, true);
  await page.getByRole('button', { name: 'Show terminal drawer' }).click();
  const shell = page.locator('[data-component="app-shell"]'),
    drawer = page.locator('[data-component="terminal-drawer"]');
  await drawer.locator('[data-tab-kind="terminal"][data-terminal-id="codex-main"] .kui-app-tab__select').click();
  const terminalInput = drawer.locator('.terminal-session:not([hidden]) .xterm-helper-textarea');
  await terminalInput.focus();
  await expect(terminalInput).toBeFocused();
  await expect(shell).toHaveAttribute('data-terminal-focus-mode', 'true');
  await expect(drawer).toHaveAttribute('data-focus-mode', 'true');
  await expect(drawer.locator('.terminal-drawer__rail')).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Exit terminal focus' })).toBeVisible();
  expect(await drawer.boundingBox()).toMatchObject({ x: 0, y: 0, width: 390, height: 844 });
  await page.screenshot({ path: testInfo.outputPath('mobile-terminal-focus-full.png') });

  await page.evaluate(() => {
    (
      window as unknown as Window & {
        __setTerminalTestViewport: (next: { left: number; top: number; width: number; height: number }) => void;
      }
    ).__setTerminalTestViewport({ left: 4, top: 18, width: 382, height: 492 });
  });
  await expect
    .poll(async () => {
      const box = await drawer.boundingBox();
      return box && { x: box.x, y: box.y, width: box.width, height: box.height, bottom: box.y + box.height };
    })
    .toEqual({ x: 4, y: 18, width: 382, height: 492, bottom: 510 });
  await expect(terminalInput).toBeFocused();
  await drawer.screenshot({ path: testInfo.outputPath('mobile-terminal-focus-keyboard.png') });

  await page.getByRole('button', { name: 'Exit terminal focus' }).click();
  await expect(shell).toHaveAttribute('data-terminal-focus-mode', 'false');
  await expect(drawer).toHaveAttribute('data-focus-mode', 'false');
  await expect(drawer.locator('.terminal-drawer__rail')).toBeVisible();
  await expect(
    drawer.locator('[data-tab-kind="terminal"][data-terminal-id="codex-main"] .kui-app-tab__select'),
  ).toBeFocused();
  await terminalInput.focus();
  await expect(drawer).toHaveAttribute('data-focus-mode', 'true');
  await page.setViewportSize({ width: 1024, height: 700 });
  await expect(shell).toHaveAttribute('data-mobile', 'false');
  await expect(shell).toHaveAttribute('data-terminal-focus-mode', 'false');
  await expect(drawer).toHaveAttribute('data-focus-mode', 'false');
});

test('exposes the phone text-size control in drawer focus mode, cycling with a toast and hiding under the keyboard (HS2-ZSFAHF)', async ({
  page,
}, testInfo) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.addInitScript(() => {
    const fake = Object.assign(new EventTarget(), { offsetLeft: 0, offsetTop: 0, width: 390, height: 844, scale: 1 });
    Object.defineProperty(window, 'visualViewport', { configurable: true, get: () => fake });
    (window as unknown as { __setVisualViewport: (height: number) => void }).__setVisualViewport = (height) => {
      fake.height = height;
      fake.dispatchEvent(new Event('resize'));
    };
  });
  await openDemoProject(page, true);
  await page.getByRole('button', { name: 'Show terminal drawer' }).click();
  const drawer = page.locator('[data-component="terminal-drawer"]');
  await drawer.locator('[data-tab-kind="terminal"][data-terminal-id="codex-main"] .kui-app-tab__select').click();
  await drawer.locator('.terminal-session:not([hidden]) .xterm-helper-textarea').focus();
  await expect(drawer).toHaveAttribute('data-focus-mode', 'true');
  // The drawer focus mode now offers the same text-size control as the magnified terminal (HS2-ZSFAHF).
  const textSize = drawer.getByRole('button', { name: /^Text size: \d+ columns/ });
  await expect(textSize).toBeVisible();
  await drawer.screenshot({ path: testInfo.outputPath('drawer-focus-text-size.png') });
  const before = await textSize.getAttribute('data-columns');
  await textSize.click();
  await expect(page.locator('.app-toast')).toHaveText(/^Terminal text size: \d+ columns$/);
  await expect(textSize).not.toHaveAttribute('data-columns', before!);
  // Hidden while the virtual keyboard is presented, then shown again when it hides.
  await page.evaluate(() => {
    (window as unknown as { __setVisualViewport: (h: number) => void }).__setVisualViewport(420);
  });
  await expect(textSize).toBeHidden();
  await page.evaluate(() => {
    (window as unknown as { __setVisualViewport: (h: number) => void }).__setVisualViewport(844);
  });
  await expect(textSize).toBeVisible();
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
  await expect(sidebar).toHaveAttribute('data-presentation', 'overlay');
  await expect(inspector).toHaveAttribute('data-presentation', 'overlay');
  await expect(scrim).toHaveCount(0);

  // Open the project sidebar as an overlay.
  await page.getByRole('button', { name: 'Show project sidebar' }).click();
  await expect(sidebar).toHaveAttribute('data-collapsed', 'false');
  await expect(scrim).toBeVisible();
  // The sidebar overlays the main column rather than sitting beside it.
  await expect(sidebar).toHaveCSS('position', 'absolute');
  await expect(sidebar.locator('.kui-resizable-region__content')).toHaveCSS('transform', 'none');

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
  await expect(inspector.locator('.kui-resizable-region__content')).toHaveCSS('transform', 'none');
  await page.screenshot({ path: '/private/tmp/hs2-zk51wp-mobile-inspector-overlay.png', fullPage: true });

  // Dismiss via the scrim strip the right inspector does not cover.
  await scrim.click({ position: { x: 10, y: 400 } });
  await expect(inspector).toHaveAttribute('data-collapsed', 'true');
  await expect(scrim).toHaveCount(0);
  await page.screenshot({ path: '/private/tmp/hs2-zk51wp-mobile-single-column.png', fullPage: true });
});

test('mobile side panels cover the terminal drawer and pad interactive content inside safe areas (HS2-3BVWME)', async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await openDemoProject(page, true);
  await page.evaluate(() => {
    document.documentElement.style.setProperty('--hotsheet-safe-area-top', '13px');
    document.documentElement.style.setProperty('--hotsheet-safe-area-right', '11px');
    document.documentElement.style.setProperty('--hotsheet-safe-area-bottom', '37px');
    document.documentElement.style.setProperty('--hotsheet-safe-area-left', '7px');
  });
  await page.getByRole('button', { name: 'Show terminal drawer' }).click();
  const shell = page.locator('[data-component="app-shell"]'),
    sidebarRegion = page.locator('.kui-resizable-region[data-region-id="app-sidebar"]'),
    inspectorRegion = page.locator('.kui-resizable-region[data-region-id="app-inspector"]');
  await page.getByRole('button', { name: 'Show project sidebar' }).click();
  await expect(sidebarRegion).toHaveAttribute('data-collapsed', 'false');
  await expect(sidebarRegion.locator('.kui-resizable-region__content')).toHaveCSS('transform', 'none');
  await expect
    .poll(() =>
      sidebarRegion.evaluate((node) => {
        const rect = node.getBoundingClientRect();
        return { top: rect.top, bottom: rect.bottom, viewportBottom: innerHeight };
      }),
    )
    .toEqual({ top: 0, bottom: 844, viewportBottom: 844 });
  await expect(sidebarRegion.locator('.project-sidebar')).toHaveCSS('padding-top', '13px');
  await expect(sidebarRegion.locator('.project-sidebar')).toHaveCSS('padding-bottom', '37px');
  await expect(sidebarRegion.locator('.project-sidebar')).toHaveCSS('padding-left', '7px');
  await page.screenshot({
    path: '/private/tmp/hs2-3bvwme-mobile-sidebar-full-height.png',
    fullPage: true,
    animations: 'disabled',
  });

  await page.locator('.app-shell__scrim').click({ position: { x: 380, y: 400 } });
  await page.locator('[data-ticket-slug="HS2-M1"]').click();
  await expect(inspectorRegion).toHaveAttribute('data-collapsed', 'false');
  await expect(inspectorRegion.locator('.kui-resizable-region__content')).toHaveCSS('transform', 'none');
  await expect
    .poll(() =>
      inspectorRegion.evaluate((node) => {
        const rect = node.getBoundingClientRect();
        return { top: rect.top, bottom: rect.bottom, viewportBottom: innerHeight };
      }),
    )
    .toEqual({ top: 0, bottom: 844, viewportBottom: 844 });
  await expect(inspectorRegion.locator('.ticket-inspector')).toHaveCSS('padding-top', '13px');
  await expect(inspectorRegion.locator('.ticket-inspector')).toHaveCSS('padding-bottom', '37px');
  await expect(inspectorRegion.locator('.ticket-inspector')).toHaveCSS('padding-right', '11px');
  await expect(shell).toHaveAttribute('data-mobile', 'true');
  await page.screenshot({
    path: '/private/tmp/hs2-3bvwme-mobile-inspector-full-height.png',
    fullPage: true,
    animations: 'disabled',
  });
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
  // Ordinary Select content retains its name and selected text alongside the custom project control.
  await expect(inspector.getByRole('combobox', { name: 'Category', exact: true })).toHaveValue('Bug');
  await expect(inspector.locator('wa-select[name="inspector-category"]')).toHaveJSProperty('value', 'bug');

  // Tap-away on the scrim returns to the list; the selection persists so tapping reopens it.
  await scrim.click({ position: { x: 10, y: 400 } });
  await expect(inspector).toHaveAttribute('data-collapsed', 'true');
  await expect(scrim).toHaveCount(0);
  await page.locator('[data-action="select-ticket-row"][data-ticket-slug="HS2-M1"]').click();
  await expect(inspector).toHaveAttribute('data-collapsed', 'false');
});

test('mobile replaces the project tabs and view title with select controls (HS2-4C5RM7)', async ({
  page,
}, testInfo) => {
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
  await page.getByRole('textbox', { name: /^Project folder/ }).fill('/work/demo');
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await expect(page.locator('[data-ticket-slug="HS2-M1"]')).toBeVisible();

  // The horizontal project tab strip is replaced by a project Select; no project tabs render.
  const projectSelect = page.locator('wa-select[name="mobile-project"]');
  await expect(projectSelect).toBeVisible();
  await expect(projectSelect).toHaveAttribute('value', 'demo-checkout');
  const projectCombobox = page.getByRole('combobox', { name: 'Project', exact: true });
  await expect(projectCombobox).toHaveValue('demo');
  await expect(projectCombobox).toHaveAccessibleName('Project');
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
  await expect(projectCombobox).toHaveValue('other');
  await expect(projectSelect.locator('.project-tab-bar__selected-project')).toHaveText('other');
  await expect(page.locator('[data-ticket-slug="HS2-OTHER1"]')).toBeVisible();
  await projectSelect.click();
  await projectSelect.locator('wa-option[value="demo-checkout"]').click();
  await expect(projectSelect).toHaveJSProperty('value', 'demo-checkout');
  await expect(page.locator('[data-ticket-slug="HS2-M1"]')).toBeVisible();
  await expect(projectCombobox).toHaveAccessibleName('Project');
  await expect(projectCombobox).toHaveValue('demo');
  await expect(projectSelect.locator('.project-tab-bar__selected-project')).toHaveText('demo');
  const projectGeometry = await projectSelect.evaluate((element) => ({
    host: element.getBoundingClientRect().height,
    control: element.shadowRoot!.querySelector('[part~="combobox"]')!.getBoundingClientRect().height,
  }));
  expect(Math.abs(projectGeometry.host - projectGeometry.control)).toBeLessThan(1);
  await page.screenshot({ path: testInfo.outputPath('q6-beta28-mobile-project-accessible-name.png') });
  // The same stable name survives destruction/recreation of the responsive bar.
  await page.setViewportSize({ width: 1280, height: 900 });
  await expect(page.locator('[data-tab-kind="project"]')).toHaveCount(2);
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(projectCombobox).toHaveAccessibleName('Project');
  await expect(projectSelect).toHaveJSProperty('value', 'demo-checkout');
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

for (const initialWidth of [390, 1280]) {
  test(`keeps Clear typing in search before frames from ${initialWidth}px (HS2-NNNFFR)`, async ({ page }, testInfo) => {
    await page.setViewportSize({ width: initialWidth, height: 844 });
    await openDemoProject(page);
    const editor = page.getByRole('searchbox', { name: 'Search tickets' });
    const field = page.locator('[data-token-search-id="workspace-search"]');
    const outside = page.getByRole('button', { name: 'Add project', exact: true });
    // Input tasks may arrive before the next frame. Hold only frames scheduled by
    // the Clear click so Playwright's ordinary pointer actionability still runs.
    await page.evaluate(() => {
      const request = window.requestAnimationFrame.bind(window);
      const cancel = window.cancelAnimationFrame.bind(window);
      const pending = new Map<number, FrameRequestCallback>();
      let clearing = false;
      let sequence = 0;
      document.addEventListener(
        'click',
        (event) => {
          clearing =
            event.target instanceof Element && Boolean(event.target.closest('[data-action="clear-workspace-search"]'));
        },
        true,
      );
      window.addEventListener('click', () => {
        clearing = false;
      });
      window.requestAnimationFrame = (callback) => {
        if (!clearing) return request(callback);
        pending.set(--sequence, callback);
        return sequence;
      };
      window.cancelAnimationFrame = (id) => {
        if (!pending.delete(id)) cancel(id);
      };
      Object.assign(window, {
        flushClearFrames: () => {
          const callbacks = [...pending.values()];
          pending.clear();
          for (const callback of callbacks) callback(performance.now());
        },
      });
    });
    const flushFrames = () =>
      page.evaluate(() => {
        (window as unknown as { flushClearFrames(): void }).flushClearFrames();
      });
    for (const width of [initialWidth, initialWidth === 390 ? 1280 : 390, initialWidth]) {
      await page.setViewportSize({ width, height: 844 });
      await page.getByRole('button', { name: 'Search tickets', exact: true }).click();
      await expect(editor).toBeFocused();
      for (let attempt = 0; attempt < 2; attempt += 1) {
        await editor.fill('has:attachment ');
        await expect(editor.locator('[data-component="token-search-token"]')).toHaveCount(1);
        await page.getByRole('button', { name: 'Clear search', exact: true }).click();
        // Never refocus or wait for focus before typing: c is also the page's create shortcut.
        await page.keyboard.type('continued');
        expect(
          await editor.evaluate((node) => ({ focused: document.activeElement === node, text: node.textContent })),
        ).toEqual({ focused: true, text: 'continued' });
        await expect(page.getByRole('dialog', { name: 'Create ticket', exact: true })).toHaveCount(0);
        await expect(field).toHaveAttribute('data-expanded', 'true');
        // An old clear must not collapse a newer replacement selection to a caret.
        await page.keyboard.press('ControlOrMeta+A');
        await flushFrames();
        await page.keyboard.type('next query');
        await expect(editor).toHaveText('next query');
      }
      await page.screenshot({
        path: testInfo.outputPath(`clear-focus-${initialWidth}-to-${width}.png`),
        animations: 'disabled',
      });
      await page.getByRole('button', { name: 'Clear search', exact: true }).click();
      await outside.focus();
      await flushFrames();
      await expect(outside).toBeFocused();
      await expect(field).toHaveAttribute('data-expanded', 'false');
    }
  });
}

test('blacks out the app behind a focused drawer terminal so nothing shows around it (HS2-JQPRXV)', async ({
  page,
}, testInfo) => {
  await page.addInitScript(() => {
    const viewport = Object.assign(new EventTarget(), { offsetLeft: 0, offsetTop: 0, width: 390, height: 844 });
    Object.defineProperty(window, 'visualViewport', { configurable: true, value: viewport });
    Object.defineProperty(window, '__setTerminalTestViewport', {
      configurable: true,
      value: (next: { left: number; top: number; width: number; height: number }) => {
        viewport.offsetLeft = next.left;
        viewport.offsetTop = next.top;
        viewport.width = next.width;
        viewport.height = next.height;
        viewport.dispatchEvent(new Event('resize'));
      },
    });
  });
  await page.setViewportSize({ width: 390, height: 844 });
  await openDemoProject(page, true);
  await page.getByRole('button', { name: 'Show terminal drawer' }).click();
  const drawer = page.locator('[data-component="terminal-drawer"]');
  await drawer.locator('[data-tab-kind="terminal"][data-terminal-id="codex-main"] .kui-app-tab__select').click();
  await drawer.locator('.terminal-session:not([hidden]) .xterm-helper-textarea').focus();
  await expect(drawer).toHaveAttribute('data-focus-mode', 'true');
  // Shrink the focused terminal to a sub-viewport box (as the iOS keyboard does), leaving a gap below.
  await page.evaluate(() => {
    (
      window as unknown as {
        __setTerminalTestViewport: (next: { left: number; top: number; width: number; height: number }) => void;
      }
    ).__setTerminalTestViewport({ left: 4, top: 18, width: 382, height: 492 });
  });
  await expect
    .poll(async () => {
      const box = await drawer.boundingBox();
      return box && { y: Math.round(box.y), bottom: Math.round(box.y + box.height) };
    })
    .toEqual({ y: 18, bottom: 510 });
  // A point in the gap below the terminal box must be covered by the terminal backdrop, not the app.
  const coversGap = await page.evaluate(() => {
    const focus = document.querySelector('[data-component="terminal-drawer"][data-focus-mode="true"]');
    const hit = document.elementFromPoint(195, 700);
    return Boolean(focus && hit && (hit === focus || focus.contains(hit)));
  });
  expect(coversGap).toBe(true);
  await page.screenshot({ path: testInfo.outputPath('focus-blackout.png') });
});

test('opens the drawer grid tile More actions menu from a phone tap, in front of the terminal (HS2-V2CCN6)', async ({
  browser,
}, testInfo) => {
  const context = await browser.newContext({ viewport: { width: 390, height: 844 }, hasTouch: true, isMobile: true }),
    page = await context.newPage();
  await openDemoProject(page, true);
  await page.getByRole('button', { name: 'Show terminal drawer' }).click();
  const drawer = page.locator('[data-component="terminal-drawer"]');
  await drawer.getByRole('tab', { name: 'Project grid' }).tap();
  await expect(drawer).toHaveAttribute('data-mode', 'grid');
  const more = drawer.getByRole('button', { name: 'More actions for Codex Main' });
  await more.tap();
  const menu = page.locator('[data-component="terminal-context-menu"]');
  await expect(menu).toBeVisible();
  await expect(menu.locator('wa-dropdown-item')).toHaveCount(1);
  await expect(menu.getByText('Hide Terminal')).toHaveCount(0);
  // The menu must be painted in front of the terminal grid, not merely present in the DOM.
  const inFront = await menu.evaluate((node) => {
    const box = node.getBoundingClientRect(),
      hit = document.elementFromPoint(box.left + box.width / 2, box.top + box.height / 2);
    return Boolean(hit && node.contains(hit));
  });
  expect(inFront).toBe(true);
  await page.screenshot({ path: testInfo.outputPath('drawer-grid-tile-menu-phone.png') });
  await menu.getByText('Open').tap();
  await expect(menu).toHaveCount(0);
  await expect(drawer).toHaveAttribute('data-mode', 'dedicated');
  // The same menu opens from a magnified ("zoomed") drawer tile, in front of the magnified terminal.
  // Open focuses the terminal, which enters phone focus mode; leave it to reach the tab rail.
  const exitFocus = page.getByRole('button', { name: 'Exit terminal focus' });
  if (await exitFocus.isVisible()) await exitFocus.tap();
  await drawer.getByRole('tab', { name: 'Project grid' }).tap();
  await drawer
    .locator('[data-component="terminal-tile"][data-action="preview-terminal"] .terminal-tile__preview')
    .first()
    .tap();
  const magnified = drawer.getByRole('dialog', { name: /Magnified/ });
  await expect(magnified).toBeVisible();
  await magnified.getByRole('button', { name: 'More actions for Codex Main' }).tap();
  await expect(menu).toBeVisible();
  expect(
    await menu.evaluate((node) => {
      const box = node.getBoundingClientRect(),
        hit = document.elementFromPoint(box.left + box.width / 2, box.top + box.height / 2);
      return Boolean(hit && node.contains(hit));
    }),
  ).toBe(true);
  await page.screenshot({ path: testInfo.outputPath('drawer-magnified-tile-menu-phone.png') });
  await context.close();
});
