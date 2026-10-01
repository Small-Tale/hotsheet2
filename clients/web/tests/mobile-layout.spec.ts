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
    if (path.endsWith('/checkouts/demo-checkout/tickets/git-local%3AHS2-M1'))
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
    restoreToolbar = page.locator('.kui-workbench__restore[data-panel="bottom"] .kui-floating-toolbar');
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
  const zoom = page.getByRole('toolbar', { name: 'Workspace tile zoom' });
  await expect(zoom).toBeVisible();
  // `.terminal-dashboard__zoom` is the zero-size safe-area anchor; measure the floating toolbar itself.
  const zoomBottom = await zoom.evaluate((node) => innerHeight - node.getBoundingClientRect().bottom);
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
  const sidebar = page.locator('#app-left-rail');
  const inspector = page.locator('#app-right-rail');
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
  await expect(sidebar.locator('.kui-workbench__panel-content')).toHaveCSS('transform', 'none');

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
  await expect(inspector.locator('.kui-workbench__panel-content')).toHaveCSS('transform', 'none');
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
    sidebarRegion = page.locator('#app-left-rail'),
    inspectorRegion = page.locator('#app-right-rail');
  await page.getByRole('button', { name: 'Show project sidebar' }).click();
  await expect(sidebarRegion).toHaveAttribute('data-collapsed', 'false');
  await expect(sidebarRegion.locator('.kui-workbench__panel-content')).toHaveCSS('transform', 'none');
  await expect
    .poll(() =>
      sidebarRegion.evaluate((node) => {
        const rect = node.getBoundingClientRect();
        return { top: rect.top, bottom: rect.bottom, viewportBottom: innerHeight };
      }),
    )
    .toEqual({ top: 0, bottom: 844, viewportBottom: 844 });
  // The pane surface reaches every edge; Kerf's Pane pads its pinned header, footer, and scroller slots
  // once (no app-level container padding on top of it, HS2-4A29RR).
  const sidebar = sidebarRegion.locator('[data-component="pane"]').first();
  await expect(sidebar).toHaveCSS('padding-top', '0px');
  await expect(sidebar).toHaveCSS('padding-bottom', '0px');
  await expect(sidebar.locator('> .kui-pane__header')).toHaveCSS('padding-top', '13px');
  // A header holding only a Toolbar hands its inline inset to that toolbar (8px toolbar gap + 7px inset).
  await expect(sidebar.locator('> .kui-pane__header > .kui-toolbar')).toHaveCSS('padding-left', '15px');
  await expect(sidebar.locator('> .kui-pane__footer')).toHaveCSS('padding-bottom', '37px');
  await expect(sidebar.locator('> .kui-pane__content')).toHaveCSS('padding-left', '7px');
  expect(await sidebar.locator('> .kui-pane__footer').evaluate((node) => node.getBoundingClientRect().bottom)).toBe(
    844,
  );
  await page.screenshot({
    path: '/private/tmp/hs2-3bvwme-mobile-sidebar-full-height.png',
    fullPage: true,
    animations: 'disabled',
  });

  await page.locator('.app-shell__scrim').click({ position: { x: 380, y: 400 } });
  await page.locator('[data-ticket-slug="HS2-M1"]').click();
  await expect(inspectorRegion).toHaveAttribute('data-collapsed', 'false');
  await expect(inspectorRegion.locator('.kui-workbench__panel-content')).toHaveCSS('transform', 'none');
  await expect
    .poll(() =>
      inspectorRegion.evaluate((node) => {
        const rect = node.getBoundingClientRect();
        return { top: rect.top, bottom: rect.bottom, viewportBottom: innerHeight };
      }),
    )
    .toEqual({ top: 0, bottom: 844, viewportBottom: 844 });
  // The inspector is a Kerf Pane inside its app card (HS2-RWGQWN). The card is the rail's child, so
  // Kerf pads the rail's panel content for the device insets once (top, right, and the home
  // indicator) and the card and its Pane carry none of their own (HS2-4A29RR).
  const panelContent = inspectorRegion.locator('> .kui-workbench__panel-content'),
    inspector = inspectorRegion.locator('.ticket-inspector'),
    inspectorPane = inspector.locator('[data-component="pane"]').first();
  await expect(panelContent).toHaveCSS('padding-top', '13px');
  await expect(panelContent).toHaveCSS('padding-right', '11px');
  await expect(panelContent).toHaveCSS('padding-bottom', '37px');
  await expect(inspector).toHaveCSS('padding-top', '0px');
  await expect(inspector).toHaveCSS('padding-bottom', '0px');
  await expect(inspector).toHaveCSS('padding-right', '0px');
  await expect(inspectorPane.locator('> .kui-pane__header')).toHaveCSS('padding-top', '0px');
  await expect(inspectorPane.locator('> .kui-pane__content')).toHaveCSS('padding-bottom', '0px');
  expect(
    await inspectorPane.locator('> .kui-pane__content').evaluate((node) => node.getBoundingClientRect().bottom),
  ).toBe(844 - 37);
  await expect(shell).toHaveAttribute('data-mobile', 'true');
  await page.screenshot({
    path: '/private/tmp/hs2-3bvwme-mobile-inspector-full-height.png',
    fullPage: true,
    animations: 'disabled',
  });
});

test('mobile ticket scrollers reach the screen bottom and inset their content for the home indicator (HS2-4A29RR)', async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await openDemoProject(page, true, 30);
  await expect(page.locator('[data-ticket-slug="HS2-M1"]')).toBeVisible();
  await page.evaluate(() => {
    document.documentElement.style.setProperty('--hotsheet-safe-area-top', '47px');
    document.documentElement.style.setProperty('--hotsheet-safe-area-bottom', '34px');
  });
  const geometry = (selector: string) =>
    page
      .locator(selector)
      .first()
      .evaluate((node) => {
        const rect = node.getBoundingClientRect(),
          style = getComputedStyle(node);
        return {
          top: rect.top,
          bottom: rect.bottom,
          paddingTop: style.paddingTop,
          paddingBottom: style.paddingBottom,
          scrollPaddingBottom: style.scrollPaddingBottom,
        };
      });
  // The top bar is the Workbench work area's Pane header, which sits under the translucent status
  // bar: the header surface starts at the edge and pads by the inset, and the toolbar inside it
  // keeps Kerf's own 8px gap above the control band (HS2-P289N2).
  const toolbar = page.locator('[data-component="toolbar"][aria-label="Workspace toolbar"]').first(),
    paneHeader = toolbar.locator('xpath=ancestor::*[contains(@class, "kui-pane__header")][1]');
  expect(
    await paneHeader.evaluate((node) => ({
      top: node.getBoundingClientRect().top,
      paddingTop: getComputedStyle(node).paddingTop,
    })),
  ).toEqual({ top: 0, paddingTop: '47px' });
  expect(
    await toolbar.evaluate((node) => ({
      top: node.getBoundingClientRect().top,
      paddingTop: getComputedStyle(node).paddingTop,
    })),
  ).toEqual({ top: 47, paddingTop: '8px' });
  expect(
    await toolbar
      .locator('button')
      .first()
      .evaluate((node) => node.getBoundingClientRect().top),
  ).toBe(57);
  // The list scroller reaches the bottom edge with the inset inside its padding and scroll padding.
  expect(await geometry('.app-shell__workspace')).toMatchObject({
    bottom: 844,
    paddingBottom: '45.2px',
    scrollPaddingBottom: '34px',
  });
  await page.locator('.app-shell__workspace').evaluate((node) => {
    node.scrollTop = node.scrollHeight;
  });
  await page.screenshot({ path: '/private/tmp/hs2-4a29rr-mobile-list-bottom.png', animations: 'disabled' });
  // Each board column scroller does the same.
  await page.getByLabel('Columns view').click();
  await expect(page.locator('.ticket-board-column__tickets').first()).toBeVisible();
  expect(await geometry('.ticket-board-column__tickets')).toMatchObject({
    bottom: 844,
    paddingBottom: '50px',
    scrollPaddingBottom: '34px',
  });
  // Page to the populated Started column and scroll it to its end for the capture.
  await page
    .locator('.ticket-board-column')
    .nth(1)
    .evaluate((node) => {
      node.scrollIntoView({ inline: 'start', block: 'nearest', behavior: 'instant' });
      const tickets = node.querySelector<HTMLElement>('.ticket-board-column__tickets')!;
      tickets.scrollTop = tickets.scrollHeight;
    });
  await page.screenshot({ path: '/private/tmp/hs2-4a29rr-mobile-board-bottom.png', animations: 'disabled' });
  // An expanded terminal drawer owns the bottom edge, so the scrollers return to their ordinary padding.
  await page.getByRole('button', { name: 'Show terminal drawer' }).click();
  await expect(page.locator('#app-bottom-drawer[data-collapsed="false"]')).toBeVisible();
  expect(await geometry('.ticket-board-column__tickets')).toMatchObject({ paddingBottom: '16px' });
  await page.getByLabel('List view').click();
  expect(await geometry('.app-shell__workspace')).toMatchObject({ paddingBottom: '11.2px' });
});

test('mobile keyboard shortcuts toggle mutually exclusive sidebar overlays without changing desktop preferences (HS2-KN79XP)', async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await openDemoProject(page);
  const modifier = await page.evaluate(() =>
    /macintosh|mac os|iphone|ipad|ipod/i.test(navigator.userAgent) ? 'Meta' : 'Control',
  );
  const sidebar = page.locator('#app-left-rail'),
    inspector = page.locator('#app-right-rail'),
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

test('mobile pages the columns view one snapped column at a time and keeps the board on desktop (HS2-ZYJMDP)', async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await openDemoProject(page);
  await expect(page.locator('[data-ticket-slug="HS2-M1"]')).toBeVisible();
  const board = page.locator('[data-component="ticket-board"]');
  const columns = board.locator('[data-component="ticket-board-column"]');

  // The Columns toggle is offered on mobile and presents the paged board edge to edge.
  await page.getByRole('button', { name: 'Columns view' }).click();
  await expect(board).toHaveAttribute('data-layout', 'paged');
  await expect(page.locator('[data-component="ticket-list"]')).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Columns view' })).toHaveAttribute('aria-pressed', 'true');
  await expect(page.locator('.app-shell__workspace')).toHaveAttribute('data-presentation', 'edge-to-edge');
  await expect(board).toHaveCSS('scroll-snap-type', 'x mandatory');
  const geometry = await board.evaluate((element) => {
    const bounds = element.getBoundingClientRect(),
      cells = [...element.querySelectorAll('[data-component="ticket-board-column"]')].map((cell) =>
        cell.getBoundingClientRect(),
      );
    return { board: bounds, first: cells[0], second: cells[1], overflow: element.scrollWidth - element.clientWidth };
  });
  // One column nearly fills the board while the next one peeks in at the trailing edge.
  expect(geometry.first.width).toBeGreaterThan(geometry.board.width * 0.85);
  expect(geometry.first.width).toBeLessThan(geometry.board.width);
  expect(geometry.second.left).toBeLessThan(geometry.board.right);
  expect(geometry.overflow).toBeGreaterThan(geometry.first.width);

  // A partial horizontal scroll settles on the nearest column start once it is released: that column's
  // leading edge rests at the board's 8px scroll padding.
  const columnInset = (index: number) =>
    board.evaluate(
      (element, target) =>
        Math.round(
          element.querySelectorAll('[data-component="ticket-board-column"]')[target].getBoundingClientRect().left -
            element.getBoundingClientRect().left,
        ),
      index,
    );
  const scrollLeft = () => board.evaluate((element) => Math.round(element.scrollLeft));
  await board.hover();
  await page.mouse.wheel(Math.round(geometry.first.width * 0.7), 0);
  await expect.poll(() => columnInset(1), { timeout: 5000 }).toBe(8);
  const second = await scrollLeft();
  expect(second).toBeGreaterThan(0);
  await page.mouse.wheel(-Math.round(geometry.first.width * 0.3), 0);
  await expect.poll(() => columnInset(1), { timeout: 5000 }).toBe(8);
  await page.mouse.wheel(-Math.round(geometry.first.width * 0.8), 0);
  await expect.poll(scrollLeft, { timeout: 5000 }).toBe(0);
  await expect.poll(() => columnInset(0), { timeout: 5000 }).toBe(8);
  await expect(columns.first()).toBeInViewport({ ratio: 0.95 });

  // Tapping a ticket inside the paged board still opens the mobile inspector overlay.
  await board.locator('[data-action="select-ticket-row"][data-ticket-slug="HS2-M1"]').scrollIntoViewIfNeeded();
  await board.locator('[data-action="select-ticket-row"][data-ticket-slug="HS2-M1"]').click();
  await expect(page.locator('#app-right-rail')).toHaveAttribute('data-collapsed', 'false');
  await page.locator('.app-shell__scrim').click({ position: { x: 8, y: 400 } });

  // Growing to desktop keeps the board preference in the side-by-side grid layout, and back again.
  await page.setViewportSize({ width: 1280, height: 800 });
  await expect(board).toHaveAttribute('data-layout', 'grid');
  await expect(board).toHaveCSS('scroll-snap-type', 'none');
  await expect(page.getByRole('button', { name: 'Columns view' })).toHaveAttribute('aria-pressed', 'true');
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(board).toHaveAttribute('data-layout', 'paged');
  await page.getByRole('button', { name: 'List view' }).click();
  await expect(page.locator('[data-component="ticket-list"]')).toBeVisible();
  await expect(board).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'List view' })).toHaveAttribute('aria-pressed', 'true');
});

test('tapping a ticket auto-opens the inspector overlay, and tap-away returns to the list (HS2-N7RPFP)', async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await openDemoProject(page);
  const inspector = page.locator('#app-right-rail');
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
  await expect(viewSelect.locator('xpath=..')).toHaveAttribute('data-component', 'toolbar-control-group');
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
  await expect(page.locator('.workspace-header__identity')).toHaveCount(0);
  // 2 & 3. The project and view selects are borderless (their combobox part has no border).
  // Kerf's toolbar-borderless Select presentation keeps a transparent 1px border in the combobox
  // part, so "borderless" means no visible border, not a zero border width.
  const comboBorderless = (name: string) =>
    page.locator(`wa-select[name="${name}"]`).evaluate((node) => {
      const part = (node as HTMLElement).shadowRoot?.querySelector('[part~="combobox"]');
      if (!part) return 'no-part';
      const style = getComputedStyle(part);
      return style.borderTopWidth === '0px' || style.borderTopColor === 'rgba(0, 0, 0, 0)';
    });
  expect(await comboBorderless('mobile-project')).toBe(true);
  expect(await comboBorderless('mobile-view')).toBe(true);
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
  const sidebar = page.locator('#app-left-rail');
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
    inspector = page.locator('#app-right-rail'),
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
      group = page.locator('.ticket-search-field').filter({ has: field });
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
  const sidebar = page.locator('#app-left-rail');
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
            event.target instanceof Element && Boolean(event.target.closest('[data-action="clear-ticket-search"]'));
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

test('keeps every tab strip horizontally scrollable only (HS2-QG4K9W)', async ({ browser }) => {
  for (const width of [390, 1440]) {
    const phone = width < 500,
      context = await browser.newContext({ viewport: { width, height: 844 }, hasTouch: phone, isMobile: phone }),
      page = await context.newPage();
    await openDemoProject(page, true);
    await page.getByRole('button', { name: 'Show terminal drawer' }).click();
    const drawerTabs = page.locator('.terminal-drawer__views .kui-tab-bar__tabs');
    await expect(drawerTabs).toBeVisible();
    // The drawer strip grows with its tabs rather than clipping them into a vertical scroll range.
    expect(await drawerTabs.evaluate((element) => element.scrollHeight - element.clientHeight)).toBeLessThanOrEqual(0);
    await page.locator('[data-ticket-slug]').first().click();
    await expect(page.locator('.ticket-inspector__tabs .kui-tab-bar__tabs').first()).toBeVisible();
    const strips = page.locator('.kui-tab-bar__tabs');
    expect(await strips.count()).toBeGreaterThanOrEqual(phone ? 2 : 3);
    for (const strip of await strips.all()) {
      if (!(await strip.isVisible())) continue;
      expect(await strip.evaluate((element) => getComputedStyle(element).overflowY)).toBe('hidden');
      const box = (await strip.boundingBox())!;
      await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
      await page.mouse.wheel(0, 40);
      await expect.poll(() => strip.evaluate((element) => element.scrollTop)).toBe(0);
    }
    await context.close();
  }
});

test('resizing the phone drawer never focuses its terminal, only a tap does (HS2-YD7RZ7)', async ({
  browser,
}, testInfo) => {
  const context = await browser.newContext({ viewport: { width: 390, height: 844 }, hasTouch: true, isMobile: true }),
    page = await context.newPage();
  await openDemoProject(page, true);
  await page.getByRole('button', { name: 'Show terminal drawer' }).click();
  const shell = page.locator('[data-component="app-shell"]'),
    drawer = page.locator('[data-component="terminal-drawer"]'),
    terminalInput = drawer.locator('.terminal-session:not([hidden]) .xterm-helper-textarea');
  // Opening a terminal tab on a phone does not auto-focus it either.
  await drawer.locator('[data-tab-kind="terminal"][data-terminal-id="codex-main"] .kui-app-tab__select').tap();
  await expect(drawer.locator('.terminal-session:not([hidden]) [data-geometry-ready="true"]')).toHaveCount(1);
  await expect(terminalInput).not.toBeFocused();
  await expect(shell).toHaveAttribute('data-terminal-focus-mode', 'false');

  const handle = page.locator('#app-bottom-drawer [data-kui-resize-handle]').first(),
    before = (await drawer.boundingBox())!.height;
  for (const delta of [-120, 80]) {
    const box = (await handle.boundingBox())!,
      x = box.x + box.width / 2,
      y = box.y + box.height / 2;
    await page.mouse.move(x, y);
    await page.mouse.down();
    await page.mouse.move(x, y + delta / 2);
    await page.mouse.move(x, y + delta);
    await page.mouse.up();
    // Let the settled refit and the drawer-resize-end focus path run.
    await page.waitForTimeout(400);
    await expect(terminalInput).not.toBeFocused();
    await expect(shell).toHaveAttribute('data-terminal-focus-mode', 'false');
    await expect(drawer.locator('.terminal-drawer__rail')).toBeVisible();
  }
  expect((await drawer.boundingBox())!.height).not.toBe(before);
  await page.screenshot({ path: testInfo.outputPath('phone-drawer-after-resize.png') });

  // A direct tap on the terminal still focuses it and enters phone focus mode.
  await drawer.locator('.terminal-session:not([hidden]) .terminal-viewport').first().tap();
  await expect(terminalInput).toBeFocused();
  await expect(shell).toHaveAttribute('data-terminal-focus-mode', 'true');
  await context.close();
});

test('phone terminal drawer keeps the home-indicator inset below every content view and grows its minimum (HS2-ZEC4QV)', async ({
  page,
}, testInfo) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await openDemoProject(page, true);
  await page.evaluate(() => {
    document.documentElement.style.setProperty('--hotsheet-safe-area-bottom', '34px');
  });
  await page.getByRole('button', { name: 'Show terminal drawer' }).click();
  const drawer = page.locator('[data-component="terminal-drawer"]'),
    handle = page.getByRole('separator', { name: 'Resize Terminal drawer' }),
    gap = (selector: string) =>
      page
        .locator(selector)
        .evaluateAll((nodes) => nodes.map((node) => innerHeight - node.getBoundingClientRect().bottom));
  await expect(page.locator('.app-shell')).toHaveAttribute('data-terminal-drawer-transitioning', 'false');
  // Kerf's Workbench pads the drawer's panel content by the bottom inset; the app adds no second one.
  await expect(page.locator('#app-bottom-drawer .kui-workbench__panel-content')).toHaveCSS('padding-bottom', '34px');
  await expect(drawer).toHaveCSS('padding-bottom', '0px');
  await expect.poll(() => gap('[data-component="terminal-drawer"] .terminal-drawer__content')).toEqual([34]);
  // The resize range reflects the padded minimum (228px usable + 34px inset).
  await expect(handle).toHaveAttribute('aria-valuemin', '262');
  await expect(handle).toHaveAttribute('aria-valuenow', '320');
  // Vertically scrolling grid (two across): the scroller itself ends above the inset.
  const grid = drawer.locator('.terminal-dashboard'),
    zoomIn = grid.getByRole('button', { name: /^Zoom in/ });
  await expect(grid).toHaveAttribute('data-fit', '2');
  await expect.poll(() => gap('[data-component="terminal-drawer"] .terminal-dashboard__content')).toEqual([34]);
  const zoomToolbar = grid.getByRole('toolbar', { name: 'Workspace tile zoom' });
  // The zoom toolbar hangs off the padded edge (its own 16px inset), not below it.
  expect((await gap('[data-component="terminal-drawer"] [aria-label="Workspace tile zoom"]'))[0]).toBeCloseTo(50, 0);
  await expect(zoomToolbar).toBeVisible();
  await page.screenshot({ path: testInfo.outputPath('drawer-grid-two-across.png'), animations: 'disabled' });
  // One-row grid (horizontal scrolling): every tile stops above the inset.
  await zoomIn.click();
  await expect(grid).toHaveAttribute('data-fit', '1');
  await expect
    .poll(async () => (await gap('[data-component="terminal-drawer"] .terminal-tile')).every((value) => value >= 34))
    .toBe(true);
  await page.screenshot({ path: testInfo.outputPath('drawer-grid-one-row.png'), animations: 'disabled' });
  // A single terminal view ends above the inset too.
  await drawer.locator('[data-tab-kind="terminal"][data-terminal-id="codex-main"] .kui-app-tab__select').click();
  await expect(drawer.locator('.terminal-session:not([hidden]) [data-geometry-ready="true"]')).toHaveCount(1);
  await expect.poll(() => gap('[data-component="terminal-drawer"] .terminal-session:not([hidden])')).toEqual([34]);
  await page.screenshot({ path: testInfo.outputPath('drawer-single-terminal.png'), animations: 'disabled' });
  // Keyboard shrinking stops at the padded minimum, and one more step collapses the drawer.
  await handle.focus();
  for (let step = 0; step < 4; step += 1) await page.keyboard.press('ArrowDown');
  await expect(handle).toHaveAttribute('aria-valuenow', '262');
  await page.keyboard.press('ArrowDown');
  await expect(page.getByRole('button', { name: 'Show terminal drawer' })).toBeVisible();
});
