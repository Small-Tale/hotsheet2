import { expect, type Locator, type Page, test } from '@playwright/test';

import { ticketsJson } from './ticket-page-fixture';

// HS2-EYD24K / HS2-QV814W: a tile can cold-mount a full instance within the warm cap. A drawer visit
// moves that same emulator and socket between the grid and the session without replay.
const TERMINALS = ['one', 'two', 'three'].map((name) => `demo-${name}`);

async function openProject(page: Page, width: number, terminals = TERMINALS, warmLimit?: number) {
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
      return route.fulfill({ json: terminals.map((id) => ({ id, alive: true, busy: false, cwd: '/work/demo' })) });
    if (path.endsWith('/terminal-limits') && warmLimit !== undefined)
      return route.fulfill({
        json: {
          defaults: { history_lines: 5000, replay_bytes: 2097152, warm_terminals: 12 },
          configured: { history_lines: null, replay_bytes: null, warm_terminals: warmLimit },
          effective: { history_lines: 5000, replay_bytes: 2097152, warm_terminals: warmLimit },
          physical_memory_bytes: 17179869184,
        },
      });
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

type MoveSocket = { url: string; sent: unknown[]; dispatchEvent: (event: Event) => boolean };
type ResizeClaim = { viewer_id: string; cols: number; rows: number; focus: boolean; visible: boolean };

/** The newest PTY sizing claim the page sent on `id`'s socket. */
const latestClaim = (page: Page, id: string) =>
  page.evaluate((terminalId) => {
    const claims = ((window as unknown as { __moveSockets?: MoveSocket[] }).__moveSockets ?? [])
      .filter((socket) => socket.url.includes(`/terminals/${terminalId}/`))
      .flatMap((socket) => socket.sent)
      .filter((value): value is string => typeof value === 'string' && value.startsWith('{"resize"'))
      .map((value) => (JSON.parse(value) as { resize: ResizeClaim }).resize);
    return claims.at(-1);
  }, id);

/** Reports a PTY size from the server, as the arbiter does after a claim (`drivenBy` names the driver). */
const reportSize = (page: Page, id: string, cols: number, rows: number, drivenBy: string) =>
  page.evaluate(
    ({ terminalId, cols, rows, drivenBy }) => {
      for (const socket of ((window as unknown as { __moveSockets?: MoveSocket[] }).__moveSockets ?? []).filter(
        (candidate) => candidate.url.includes(`/terminals/${terminalId}/`),
      ))
        socket.dispatchEvent(
          new MessageEvent('message', { data: JSON.stringify({ pty_size: { cols, rows }, driven_by: drivenBy }) }),
        );
    },
    { terminalId: id, cols, rows, drivenBy },
  );

/** The `{cols, rows}` of a `data-grid-size` value. */
const claimOf = (grid: string) => {
  const [cols, rows] = grid.split('x').map(Number);
  return { cols, rows };
};

/**
 * A live instance in a grid tile renders and claims the scaled 80×24 grid, never a small fitted PTY
 * (HS2-JQCGQF). Its screen stays inside the card and fills it along one axis.
 */
async function expectScaled80x24Tile(page: Page, tile: Locator, id: string) {
  const viewport = tile.locator('[data-presentation="tile"]');
  await expect(viewport).toHaveAttribute('data-geometry-ready', 'true');
  await expect(viewport).toHaveAttribute('data-grid-size', '80x24');
  await expect(viewport).toHaveAttribute('data-sizing-focus', 'true');
  await expect(viewport).toHaveAttribute('data-font-size', '24');
  await expect.poll(async () => (await latestClaim(page, id)) ?? null).toMatchObject({ cols: 80, rows: 24 });
  await expect
    .poll(() =>
      viewport.evaluate((element) => {
        const frame = element.parentElement!.getBoundingClientRect(),
          screen = element.querySelector('.xterm-screen')!.getBoundingClientRect(),
          width = screen.width / frame.width,
          height = screen.height / frame.height;
        return width <= 1.01 && height <= 1.01 && Math.max(width, height) > 0.95;
      }),
    )
    .toBe(true);
}

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
    await expect(dedicated.locator('.xterm-rows')).toContainText('demo-one line');
    const [opened] = await emulatorOf(page, 'demo-one'),
      sockets = await socketsFor(page, 'demo-one');
    expect(opened).toMatchObject({ inSession: true, presentation: 'drawer' });
    await expect(dedicated).toHaveAttribute('data-geometry-ready', 'true');
    const drawerFit = (await dedicated.getAttribute('data-grid-size'))!;
    expect(drawerFit).not.toBe('80x24');

    for (let round = 0; round < 2; round += 1) {
      // The drawer's project grid shows the same emulator inside the tile, scaled and inert.
      await drawer.getByRole('button', { name: 'Project Grid' }).click();
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
      await expectScaled80x24Tile(page, tile, 'demo-one');
      // Other terminals continue painting; a cold full instance can scroll its initial READY line away.
      await expect(drawer.locator('[data-terminal-key="demo-checkout:demo-two"] .xterm-rows').last()).toContainText(
        'demo-two line 30',
      );
      if (round === 0)
        await page.screenshot({
          path: `target/visual-captures/hs2-eyd24k-drawer-grid-tile-${String(width)}.png`,
          animations: 'disabled',
        });

      // Back to the terminal tab: the drawer takes the same emulator back, with no reconnect,
      // and refits and claims its own grid rather than keeping the tile's 80×24 (HS2-JQCGQF).
      await drawer.getByRole('tab', { name: 'Demo One', exact: true }).click();
      await expect(dedicated).toHaveAttribute('data-connection', 'connected');
      await expect(dedicated).toHaveAttribute('data-grid-size', drawerFit);
      await expect.poll(async () => (await latestClaim(page, 'demo-one')) ?? null).toMatchObject(claimOf(drawerFit));
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
    // Every width claims the same scaled 80×24 grid, never the tile's small cell count (HS2-JQCGQF).
    await expectScaled80x24Tile(page, workspaceTile, 'demo-one');
    expect(await emulatorOf(page, 'demo-one')).toEqual([
      { tag: opened.tag, inTile: true, inSession: false, presentation: 'tile', mode: 'interactive' },
    ]);
    expect(await socketsFor(page, 'demo-one')).toBe(sockets);
    await workspaceTile.click();
    const magnified = page.getByRole('dialog', { name: /Magnified Demo One/i }),
      magnifiedViewport = magnified.locator('[data-terminal-id="demo-one"][data-display-mode="interactive"]');
    await expect(magnifiedViewport).toHaveAttribute('data-connection', 'connected');
    await expect(magnifiedViewport).toHaveAttribute('data-geometry-ready', 'true');
    await expect(magnifiedViewport).toHaveAttribute('data-grid-size', '80x24');
    expect(await magnifiedViewport.locator('.xterm').getAttribute('data-move-tag')).toBe(opened.tag);
    expect(await socketsFor(page, 'demo-one')).toBe(sockets);
    await magnified.screenshot({ path: `target/visual-captures/hs2-sa08xa-workspace-magnified-${String(width)}.png` });
    if (width === 390) await magnified.getByRole('button', { name: 'Close Demo One' }).click();
    else await magnified.click({ position: { x: 2, y: 2 } });
    await expect(magnified).toBeHidden();
    await expect(workspaceTile.locator('[data-presentation="tile"]')).toHaveCount(1);
    expect((await emulatorOf(page, 'demo-one'))[0].tag).toBe(opened.tag);
    expect(await socketsFor(page, 'demo-one')).toBe(sockets);
    // Grid to magnified to grid: the tile claims its scaled 80×24 again.
    await expectScaled80x24Tile(page, workspaceTile, 'demo-one');
    await page.screenshot({
      path: `target/visual-captures/hs2-eyd24k-workspace-grid-tile-${String(width)}.png`,
      animations: 'disabled',
    });
    if (width === 1280) {
      await workspace.getByRole('button', { name: 'Enter workspace grid full screen' }).click();
      const zoomIn = workspace.getByRole('button', { name: /Zoom In, Fit Fewer Items/ });
      for (let step = 0; step < 5 && (await zoomIn.isEnabled()); step += 1) await zoomIn.click();
      await expect(workspaceTile.locator('[data-presentation="tile"] .xterm-rows')).toContainText('demo-one READY');
    }
    await workspaceTile.screenshot({ path: test.info().outputPath(`hs2-r1v4mj-workspace-tile-${String(width)}.png`) });
  });

for (const width of [1280, 390])
  test(`cold grid tiles use full instances only within the warm cap at ${String(width)}px`, async ({ page }) => {
    const terminals = ['one', 'two', 'three', 'four', 'five'].map((name) => `demo-${name}`);
    await openProject(page, width, terminals, 2);
    const drawer = page.locator('[data-component="terminal-drawer"]');
    await expect(drawer).toHaveAttribute('data-mode', 'grid');
    const tiles = drawer.locator('[data-component="terminal-tile"]');
    await expect(tiles).toHaveCount(5);
    for (const id of terminals) {
      const tile = drawer.locator(`[data-component="terminal-tile"][data-terminal-key="demo-checkout:${id}"]`);
      await tile.scrollIntoViewIfNeeded();
      await expect(tile.locator('.xterm-rows')).toContainText(`${id} READY`);
    }
    const full = drawer.locator('[data-component="terminal-tile"] [data-presentation="tile"]');
    await expect(full).toHaveCount(2);
    expect(await socketsFor(page, 'demo-one')).toBe(1);
    expect(await socketsFor(page, 'demo-three')).toBe(1);
    const first = (await emulatorOf(page, 'demo-one'))[0];
    expect(first).toMatchObject({ inTile: true, presentation: 'tile', mode: 'interactive' });
    // A cold-mounted full instance claims the same scaled 80×24 grid as a moved one (HS2-JQCGQF).
    await expectScaled80x24Tile(page, drawer.locator('[data-terminal-key="demo-checkout:demo-one"]'), 'demo-one');
    const firstSockets = await socketsFor(page, 'demo-one');
    const firstTile = drawer.locator('[data-terminal-key="demo-checkout:demo-one"]');
    await firstTile.click();
    const magnified = page.getByRole('dialog', { name: /Magnified Demo One/i }),
      magnifiedViewport = magnified.locator('[data-terminal-id="demo-one"][data-display-mode="interactive"]');
    await expect(magnifiedViewport).toHaveAttribute('data-connection', 'connected');
    await expect(magnifiedViewport).toHaveAttribute('data-geometry-ready', 'true');
    await expect(magnifiedViewport).toHaveAttribute('data-grid-size', '80x24');
    expect(await magnifiedViewport.locator('.xterm').getAttribute('data-move-tag')).toBe(first.tag);
    expect(await socketsFor(page, 'demo-one')).toBe(firstSockets);
    await magnified.screenshot({ path: `target/visual-captures/hs2-sa08xa-drawer-magnified-${String(width)}.png` });
    if (width === 390) await magnified.getByRole('button', { name: 'Close Demo One' }).click();
    else await magnified.click({ position: { x: 2, y: 2 } });
    await expect(magnified).toBeHidden();
    await expect(firstTile.locator('[data-presentation="tile"]')).toHaveCount(1);
    expect((await emulatorOf(page, 'demo-one'))[0].tag).toBe(first.tag);
    expect(await socketsFor(page, 'demo-one')).toBe(firstSockets);
    await firstTile.click();
    const reopened = page.getByRole('dialog', { name: /Magnified Demo One/i });
    if (width === 390) {
      await reopened.getByRole('button', { name: 'More Actions for Demo One' }).click();
      await page.locator('[data-context-menu="terminal"]').getByText('Open', { exact: true }).click();
    } else await reopened.getByRole('button', { name: 'Open Demo One in Project Terminal Drawer' }).click();
    const fromMagnified = drawer.locator('[data-component="terminal-session"] [data-terminal-id="demo-one"]');
    await expect(fromMagnified).toHaveAttribute('data-connection', 'connected');
    expect((await emulatorOf(page, 'demo-one'))[0].tag).toBe(first.tag);
    expect(await socketsFor(page, 'demo-one')).toBe(firstSockets);
    await drawer.getByRole('button', { name: 'Project Grid' }).click();
    await expect(firstTile.locator('[data-presentation="tile"]')).toHaveCount(1);
    const overflow = drawer.locator('[data-terminal-id="demo-three"][data-display-mode="scaled-preview"]');
    await expect(overflow).toHaveCount(1);
    await tiles.first().scrollIntoViewIfNeeded();
    await page.screenshot({
      path: `target/visual-captures/hs2-qv814w-cold-grid-${String(width)}.png`,
      animations: 'disabled',
    });

    await drawer.getByRole('tab', { name: 'Demo One', exact: true }).click();
    const dedicated = drawer.locator('[data-component="terminal-session"] [data-terminal-id="demo-one"]');
    await expect(dedicated).toHaveAttribute('data-connection', 'connected');
    await expect(dedicated).toHaveAttribute('aria-label', 'Demo One interactive terminal');
    expect(await emulatorOf(page, 'demo-one')).toEqual([
      { ...first, inTile: false, inSession: true, presentation: 'drawer' },
    ]);
    expect(await socketsFor(page, 'demo-one')).toBe(1);
    await drawer.getByRole('button', { name: 'Project Grid' }).click();
    await expect(drawer.locator('[data-terminal-key="demo-checkout:demo-one"] [data-presentation="tile"]')).toHaveCount(
      1,
    );
    await drawer.getByRole('tab', { name: 'Demo One', exact: true }).click();
    expect(await emulatorOf(page, 'demo-one')).toEqual([
      { ...first, inTile: false, inSession: true, presentation: 'drawer' },
    ]);
    expect(await socketsFor(page, 'demo-one')).toBe(1);
  });

// HS2-JQCGQF: a drawer terminal moved into the drawer project grid and the Workspace Grid claims a
// scaled 80×24 PTY at wide, medium, and narrow widths, keeps it across a wide-narrow-wide resize,
// mirrors another device that takes the size, and refits to the drawer when it returns there.
for (const width of [1440, 1024, 390])
  test(`claims a scaled 80x24 grid in every grid tile and refits in the drawer at ${String(width)}px (HS2-JQCGQF)`, async ({
    page,
  }) => {
    await openProject(page, width, ['demo-one', 'demo-two']);
    const drawer = page.locator('[data-component="terminal-drawer"]');
    await expect(drawer).toHaveAttribute('data-mode', 'grid');
    await drawer.getByRole('tab', { name: 'Demo One', exact: true }).click();
    const dedicated = drawer.locator('[data-component="terminal-session"] [data-terminal-id="demo-one"]');
    await expect(dedicated).toHaveAttribute('data-connection', 'connected');
    await expect(dedicated).toHaveAttribute('data-geometry-ready', 'true');
    const drawerFit = (await dedicated.getAttribute('data-grid-size'))!;
    expect(drawerFit).not.toBe('80x24');
    await expect.poll(async () => (await latestClaim(page, 'demo-one')) ?? null).toMatchObject(claimOf(drawerFit));
    // The server confirms this viewer as the driver, as the arbiter does after its claim.
    const viewerId = (await latestClaim(page, 'demo-one'))!.viewer_id;
    await reportSize(page, 'demo-one', claimOf(drawerFit).cols, claimOf(drawerFit).rows, viewerId);

    // Drawer to drawer project grid to drawer.
    await drawer.getByRole('button', { name: 'Project Grid' }).click();
    const drawerTile = drawer.locator('[data-component="terminal-tile"][data-terminal-key="demo-checkout:demo-one"]');
    await expectScaled80x24Tile(page, drawerTile, 'demo-one');
    await reportSize(page, 'demo-one', 80, 24, viewerId);
    await drawer.screenshot({ path: `target/visual-captures/hs2-jqcgqf-drawer-grid-${String(width)}.png` });
    await drawer.getByRole('tab', { name: 'Demo One', exact: true }).click();
    await expect(dedicated).toHaveAttribute('data-grid-size', drawerFit);
    await expect(dedicated).not.toHaveAttribute('data-font-size', '24');
    await expect.poll(async () => (await latestClaim(page, 'demo-one')) ?? null).toMatchObject(claimOf(drawerFit));
    await reportSize(page, 'demo-one', claimOf(drawerFit).cols, claimOf(drawerFit).rows, viewerId);

    // Drawer to Workspace Grid.
    await drawer.getByRole('button', { name: 'Hide Terminal Drawer' }).click();
    await page.getByRole('button', { name: 'Workspace Grid', exact: true }).first().click();
    const workspace = page.getByRole('region', { name: 'Workspace Grid' }),
      workspaceTile = workspace.locator('[data-component="terminal-tile"][data-terminal-key="demo-checkout:demo-one"]');
    await expectScaled80x24Tile(page, workspaceTile, 'demo-one');
    await reportSize(page, 'demo-one', 80, 24, viewerId);
    await expect(workspaceTile.locator('[data-presentation="tile"]')).toHaveAttribute('data-pty-size', '80x24');
    await page.screenshot({ path: `target/visual-captures/hs2-jqcgqf-workspace-grid-${String(width)}.png` });

    // Wide to narrow to wide: the tile keeps claiming 80×24 at every width.
    for (const next of [390, 1440, width]) {
      await page.setViewportSize({ width: next, height: 900 });
      await expectScaled80x24Tile(page, workspaceTile, 'demo-one');
    }

    // Another device takes the size: the tile mirrors that PTY, scaled and contained, and does
    // not fight it with its own claim.
    await reportSize(page, 'demo-one', 132, 40, 'other-device');
    const tiled = workspaceTile.locator('[data-presentation="tile"]');
    await expect(tiled).toHaveAttribute('data-grid-size', '132x40');
    await expect(tiled).toHaveAttribute('data-driving', 'false');
    await expect
      .poll(() =>
        tiled.evaluate((element) => {
          const frame = element.parentElement!.getBoundingClientRect(),
            screen = element.querySelector('.xterm-screen')!.getBoundingClientRect();
          return screen.width <= frame.width + 1 && screen.height <= frame.height + 1;
        }),
      )
      .toBe(true);
    await reportSize(page, 'demo-one', 80, 24, viewerId);
    await expect(tiled).toHaveAttribute('data-grid-size', '80x24');

    // Workspace Grid back to the drawer: the drawer refits to its own size.
    await workspaceTile.dblclick();
    await expect(drawer).toHaveAttribute('data-mode', 'dedicated');
    await expect(dedicated).toHaveAttribute('data-connection', 'connected');
    await expect(dedicated).not.toHaveAttribute('data-presentation', 'tile');
    await expect
      .poll(async () => (await latestClaim(page, 'demo-one')) ?? null)
      .not.toMatchObject({ cols: 80, rows: 24 });
    const refit = (await dedicated.getAttribute('data-grid-size'))!;
    await expect.poll(async () => (await latestClaim(page, 'demo-one')) ?? null).toMatchObject(claimOf(refit));
  });
