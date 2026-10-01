import { expect, type Locator, type Page, test } from '@playwright/test';

// HS2-0PF13V: the drawer grid sizes its tiles from the measured drawer bounds. A re-render that
// replaced the drawer (Settings/Notifications unmount it) left the bounds observer on the detached
// node, which recorded 0x0, so the grid stayed at its minimum tile until a manual drawer resize or a
// project switch re-measured it.

const project = {
  id: 'a',
  root: '/work/a',
  name: 'Project A',
  stores: ['/work/a.hs2'],
  apiPath: '/__hotsheet/project-api/a',
  needsTicketSetup: false,
  needsHs1Migration: false,
};
const capabilities = {
  create: true,
  update: true,
  close: true,
  notes: true,
  up_next: true,
  claims: true,
  query_fields: [],
};
// Tile chrome around the 5:3 preview (terminal-grid-layout.ts).
const TILE_VERTICAL_CHROME = 24 + 43.2;
const TILE_HORIZONTAL_CHROME = 24;

async function installWorkspace(page: Page) {
  await page.addInitScript((root) => {
    localStorage.setItem('hotsheet.open-projects', JSON.stringify([root]));
    localStorage.setItem('hotsheet.workspace.active-project-root.v1', root);
    localStorage.setItem('hotsheet.terminals.drawer-open', 'true');
    localStorage.setItem('hotsheet.terminals.drawer-fit-high', '1');
    localStorage.setItem('hotsheet.project.a.terminal-drawer-selection', 'grid');
    class FakeSocket extends EventTarget {
      static CONNECTING = 0;
      static OPEN = 1;
      static CLOSING = 2;
      static CLOSED = 3;
      readyState = 0;
      binaryType = 'blob';
      constructor(public url: string) {
        super();
        setTimeout(() => {
          if (this.readyState === FakeSocket.CLOSED) return;
          this.readyState = FakeSocket.OPEN;
          this.dispatchEvent(new Event('open'));
          this.dispatchEvent(
            new MessageEvent('message', { data: new TextEncoder().encode('shell ready\r\na$ ').buffer }),
          );
        });
      }
      send(value: unknown) {
        if (typeof value !== 'string') return;
        try {
          const resize = (JSON.parse(value) as { resize?: { cols: number; rows: number; viewer_id: string } }).resize;
          if (resize)
            this.dispatchEvent(
              new MessageEvent('message', {
                data: JSON.stringify({
                  pty_size: { cols: resize.cols, rows: resize.rows },
                  driven_by: resize.viewer_id,
                }),
              }),
            );
        } catch {
          /* input */
        }
      }
      close() {
        this.readyState = FakeSocket.CLOSED;
        this.dispatchEvent(new CloseEvent('close'));
      }
    }
    Object.assign(window, { WebSocket: FakeSocket });
  }, project.root);
  await page.route('**/*', async (route) => {
    const request = route.request(),
      path = decodeURIComponent(new URL(request.url()).pathname);
    if (path === '/__hotsheet/projects/open' && request.method() === 'POST')
      return route.fulfill({ status: 201, json: project });
    const api = /\/__hotsheet\/project-api\/([^/]+)(\/.*)$/.exec(path);
    if (!api) {
      if (path.startsWith('/__hotsheet/')) return route.fulfill({ json: [] });
      return route.continue();
    }
    const rest = api[2];
    if (rest.endsWith('/ws/poll')) return route.fulfill({ json: { cursor: 1, events: [], overflow: false } });
    if (rest.endsWith('/providers'))
      return route.fulfill({
        json: [
          {
            connection_id: 'git',
            provider: 'git',
            display_name: 'Hot Sheet git',
            locator: project.stores[0],
            default: true,
            capabilities,
          },
        ],
      });
    if (rest.endsWith('/repository/status'))
      return route.fulfill({
        json: { branch: 'main', ahead: 0, behind: 0, staged: 0, unstaged: 0, untracked: 0, conflicted: 0, clean: true },
      });
    if (rest.endsWith('/terminal-settings')) return route.fulfill({ json: { inherit_global_shell_history: false } });
    if (rest.endsWith('/terminals') && request.method() === 'GET')
      return route.fulfill({ json: [{ id: 'shell', alive: true, busy: false, cwd: project.root }] });
    if (rest.endsWith('/tickets') && request.method() === 'GET')
      return route.fulfill({
        json: {
          items: [],
          counts: {
            total: 0,
            queued: 0,
            backlog: 0,
            archive: 0,
            trash: 0,
            open: 0,
            up_next: 0,
            active: 0,
            started: 0,
            completed_today: 0,
            completion_trend: [],
          },
        },
      });
    if (request.method() === 'GET') return route.fulfill({ json: [] });
    return route.fulfill({ status: 204 });
  });
}

const drawer = (page: Page) => page.locator('[data-component="terminal-drawer"]');
const tile = (page: Page) => drawer(page).locator('[data-component="terminal-tile"]').first();
const viewButton = (page: Page, label: string) => page.getByRole('button', { name: new RegExp(`^${label} view`) });

/** At drawer zoom level one, a tile fills the drawer grid's full content height (5:3 preview). */
async function expectTileFillsDrawerHeight(page: Page) {
  const content = drawer(page).locator('.terminal-drawer__content');
  await expect
    .poll(async () => {
      const [box, area] = await Promise.all([boxOf(tile(page)), boxOf(content)]);
      const expectedHeight = Math.floor(area.height) - 24;
      return {
        height: Math.abs(box.height - expectedHeight) <= 2,
        width: Math.abs(box.width - ((box.height - TILE_VERTICAL_CHROME) * 5) / 3 - TILE_HORIZONTAL_CHROME) <= 3,
      };
    })
    .toEqual({ height: true, width: true });
}

async function boxOf(locator: Locator) {
  const box = await locator.boundingBox();
  if (!box) throw new Error('element is not rendered');
  return box;
}

/** At drawer zoom level two, two tiles fit across the drawer grid's content width. */
async function expectTwoTilesAcrossDrawer(page: Page) {
  const content = drawer(page).locator('.terminal-drawer__content');
  await expect
    .poll(async () => {
      const [box, area] = await Promise.all([boxOf(tile(page)), boxOf(content)]);
      return Math.abs(box.width - Math.floor((Math.floor(area.width) - 24 - 12) / 2)) <= 2;
    })
    .toBe(true);
}

async function switchViewAndBack(page: Page, away: string, back: string) {
  await viewButton(page, away).click();
  await expect(drawer(page)).toHaveCount(0);
  await viewButton(page, back).click();
  await expect(tile(page)).toBeVisible();
}

for (const [width, height] of [
  [1440, 1000],
  [1100, 760],
] as const)
  test(`drawer grid tiles track the drawer after a view switch remounts it at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height });
    await installWorkspace(page);
    await page.goto('/?dev-review=false');
    await expect(drawer(page)).toHaveAttribute('data-mode', 'grid');
    await expect(tile(page)).toBeVisible();
    // Startup: the drawer restored open must be measured, not left at the signal's default bounds.
    await expectTileFillsDrawerHeight(page);
    const original = await boxOf(tile(page));

    // Hide + show binds the observer to the live drawer (the path the original report took).
    await page.getByRole('button', { name: 'Hide terminal drawer' }).click();
    await page.getByRole('button', { name: 'Show terminal drawer' }).click();
    await expect(tile(page)).toBeVisible();
    await expectTileFillsDrawerHeight(page);

    // Settings unmounts the drawer; returning renders a new drawer element. The old observer used to
    // record the removed node as 0x0, leaving the 160px minimum tile until a manual resize.
    await switchViewAndBack(page, 'Settings', 'List');
    await expectTileFillsDrawerHeight(page);
    expect(Math.abs((await boxOf(tile(page))).height - original.height)).toBeLessThanOrEqual(2);
    await page.screenshot({ path: `test-results/hs2-0pf13v-after-view-switch-${width}.png` });

    // A later size change that is not a drawer drag (the window narrows) still re-measures the new
    // drawer element: at zoom level two the tiles follow the drawer width.
    await drawer(page).locator('[data-action="zoom-terminal-grid"][data-zoom-direction="out"]').click();
    await expectTwoTilesAcrossDrawer(page);
    await page.setViewportSize({ width: width - 200, height });
    await expectTwoTilesAcrossDrawer(page);
    // Crossing the rail breakpoint animates the side rails; let it settle, then re-check and capture.
    await page.waitForTimeout(1000);
    await expectTwoTilesAcrossDrawer(page);
    await page.screenshot({ path: `test-results/hs2-0pf13v-after-window-resize-zoom2-${width - 200}.png` });
    await page.setViewportSize({ width, height });
    await expectTwoTilesAcrossDrawer(page);

    // Notifications remounts the drawer the same way; repeated round trips keep tracking.
    for (const view of ['Notifications', 'Settings']) {
      await switchViewAndBack(page, view, 'Columns');
      await expectTwoTilesAcrossDrawer(page);
    }
    await drawer(page).locator('[data-action="zoom-terminal-grid"][data-zoom-direction="in"]').click();
    await expectTileFillsDrawerHeight(page);
  });
