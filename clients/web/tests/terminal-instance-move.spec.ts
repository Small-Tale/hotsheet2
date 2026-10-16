import { expect, type Page, test } from '@playwright/test';

import { ticketsJson } from './ticket-page-fixture';

// HS2-EYD24K: a terminal opened in the drawer is one emulator and one socket. Switching the drawer to its
// project grid, or showing the Workspace Grid, moves that instance into the terminal's tile (scaled,
// non-interactive) instead of mounting a second emulator, and returning moves it back without a replay.
const TERMINALS = ['one', 'two', 'three'].map((name) => `demo-${name}`);

async function openProject(page: Page, width: number) {
  await page.setViewportSize({ width, height: 900 });
  await page.addInitScript(() => {
    localStorage.setItem('hotsheet.open-projects', JSON.stringify(['/work/demo']));
    localStorage.setItem('hotsheet.workspace.active-project-root.v1', '/work/demo');
    localStorage.setItem('hotsheet.terminals.drawer-open', 'true');
    localStorage.setItem('hotsheet.project.demo-checkout.terminal-drawer-selection', 'grid');
    class FakeSocket extends EventTarget {
      static CONNECTING = 0;
      static OPEN = 1;
      static CLOSING = 2;
      static CLOSED = 3;
      readyState = 0;
      binaryType = 'blob';
      sent: unknown[] = [];
      constructor(public url: string) {
        super();
        const owner = window as unknown as { __moveSockets?: FakeSocket[] };
        owner.__moveSockets ??= [];
        owner.__moveSockets.push(this);
        setTimeout(() => {
          this.readyState = 1;
          this.dispatchEvent(new Event('open'));
          const id = decodeURIComponent(url.split('/terminals/')[1].split('/')[0]);
          const lines = Array.from({ length: 30 }, (_, index) => `${id} line ${String(index + 1)}`).join('\r\n');
          this.dispatchEvent(
            new MessageEvent('message', { data: new TextEncoder().encode(`${lines}\r\n${id} READY % `).buffer }),
          );
        });
      }
      send(value: unknown) {
        this.sent.push(value);
      }
      close() {
        this.readyState = 3;
        this.dispatchEvent(new CloseEvent('close'));
      }
    }
    Object.assign(window, { WebSocket: FakeSocket });
  });
  await page.route('**/*', async (route) => {
    const request = route.request(),
      url = new URL(request.url()),
      path = url.pathname;
    if (path === '/__hotsheet/projects/open')
      return route.fulfill({
        status: 201,
        json: {
          id: 'demo-checkout',
          root: '/work/demo',
          name: 'demo',
          stores: ['/work/demo.hs2'],
          apiPath: '/__hotsheet/project-api/demo-checkout',
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
            capabilities: { create: true, update: true, query_fields: [] },
          },
        ],
      });
    if (path.endsWith('/tickets')) return route.fulfill({ json: ticketsJson(request, []) });
    if (path.endsWith('/terminals'))
      return route.fulfill({ json: TERMINALS.map((id) => ({ id, alive: true, busy: false, cwd: '/work/demo' })) });
    if (
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
}

const socketsFor = (page: Page, id: string) =>
  page.evaluate(
    (terminalId) =>
      (window as unknown as { __moveSockets?: Array<{ url: string }> }).__moveSockets?.filter((socket) =>
        socket.url.includes(`/terminals/${terminalId}/`),
      ).length ?? 0,
    id,
  );

/** Tags the live emulator of `id` wherever it is, and reports where it sits. */
function emulatorOf(page: Page, id: string) {
  return page.evaluate((terminalId) => {
    const xterms = [...document.querySelectorAll<HTMLElement>('.xterm')].filter(
      (xterm) =>
        xterm.closest<HTMLElement>('[data-component="terminal-viewport"]')?.dataset.terminalId === terminalId &&
        xterm.isConnected,
    );
    return xterms.map((xterm) => {
      xterm.dataset.moveTag ??= String(Math.random());
      const viewport = xterm.closest<HTMLElement>('[data-component="terminal-viewport"]')!;
      return {
        tag: xterm.dataset.moveTag,
        inTile: viewport.closest('[data-component="terminal-tile"]') !== null,
        inSession: viewport.closest('[data-component="terminal-session"]') !== null,
        presentation: viewport.dataset.presentation ?? 'drawer',
        mode: viewport.dataset.displayMode,
      };
    });
  }, id);
}

for (const width of [1280, 390])
  test(`moves one emulator between the drawer, its project grid, and the Workspace Grid at ${String(width)}px`, async ({
    page,
  }) => {
    await openProject(page, width);
    const drawer = page.locator('[data-component="terminal-drawer"]');
    await expect(drawer).toHaveAttribute('data-mode', 'grid');
    // Open demo-one in the drawer: its dedicated instance mounts once.
    await drawer.getByRole('tab', { name: 'Demo One', exact: true }).click();
    const dedicated = drawer.locator('[data-component="terminal-session"] [data-terminal-id="demo-one"]');
    await expect(dedicated).toHaveAttribute('data-connection', 'connected');
    await expect(dedicated.locator('.xterm-rows')).toContainText('demo-one READY');
    const [opened] = await emulatorOf(page, 'demo-one'),
      sockets = await socketsFor(page, 'demo-one');
    expect(opened).toMatchObject({ inSession: true, presentation: 'drawer' });

    for (let round = 0; round < 2; round += 1) {
      // The drawer's project grid shows the same emulator inside the tile, scaled and inert.
      await drawer.getByRole('tab', { name: 'Project grid' }).click();
      const tile = drawer.locator('[data-component="terminal-tile"][data-terminal-key="demo-checkout:demo-one"]');
      await expect(tile.locator('[data-presentation="tile"]')).toHaveCount(1);
      await expect(tile.locator('.xterm-rows')).toContainText('demo-one READY');
      expect(await emulatorOf(page, 'demo-one')).toEqual([
        { tag: opened.tag, inTile: true, inSession: false, presentation: 'tile', mode: 'interactive' },
      ]);
      const placement = await tile.locator('[data-presentation="tile"]').evaluate((viewport) => {
        const frame = viewport.parentElement!.getBoundingClientRect(),
          box = viewport.getBoundingClientRect();
        return {
          inside:
            box.left >= frame.left - 1 &&
            box.top >= frame.top - 1 &&
            box.right <= frame.right + 1 &&
            box.bottom <= frame.bottom + 1,
          fills: Math.max(box.width / frame.width, box.height / frame.height),
          inert: (viewport as HTMLElement).inert,
          // The tile's own paint guard must not hide the hosted emulator.
          painted: getComputedStyle(viewport.querySelector('.xterm-rows')!).visibility,
        };
      });
      // Letterboxed: fully inside the frame and filling it along one axis.
      expect(placement.inside).toBe(true);
      expect(placement.fills).toBeGreaterThan(0.98);
      expect(placement.inert).toBe(true);
      expect(placement.painted).toBe('visible');
      expect(await socketsFor(page, 'demo-one'), 'no new socket for the tile').toBe(sockets);
      // Other terminals keep their lightweight previews.
      await expect(
        drawer.locator('[data-terminal-id="demo-two"][data-display-mode="scaled-preview"] .xterm-rows'),
      ).toContainText('demo-two READY');
      if (round === 0)
        await page.screenshot({
          path: `target/visual-captures/hs2-eyd24k-drawer-grid-tile-${String(width)}.png`,
          animations: 'disabled',
        });

      // Back to the terminal tab: the drawer takes the same emulator back, with no reconnect.
      await drawer.getByRole('tab', { name: 'Demo One', exact: true }).click();
      await expect(dedicated).toHaveAttribute('data-connection', 'connected');
      expect(await emulatorOf(page, 'demo-one')).toEqual([
        { tag: opened.tag, inTile: false, inSession: true, presentation: 'drawer', mode: 'interactive' },
      ]);
      expect(await socketsFor(page, 'demo-one')).toBe(sockets);
    }

    // The Workspace Grid composition: hide the drawer, and the Workspace Grid tile hosts the instance.
    await drawer.getByRole('button', { name: 'Hide Terminal Drawer' }).click();
    await page.getByRole('button', { name: 'Workspace Grid', exact: true }).first().click();
    const workspace = page.getByRole('region', { name: 'Workspace Grid' }),
      workspaceTile = workspace.locator('[data-component="terminal-tile"][data-terminal-key="demo-checkout:demo-one"]');
    await expect(workspaceTile.locator('[data-presentation="tile"] .xterm-rows')).toContainText('demo-one READY');
    expect(await emulatorOf(page, 'demo-one')).toEqual([
      { tag: opened.tag, inTile: true, inSession: false, presentation: 'tile', mode: 'interactive' },
    ]);
    expect(await socketsFor(page, 'demo-one')).toBe(sockets);
    await page.screenshot({
      path: `target/visual-captures/hs2-eyd24k-workspace-grid-tile-${String(width)}.png`,
      animations: 'disabled',
    });
  });
