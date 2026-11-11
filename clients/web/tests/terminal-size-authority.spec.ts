import { type Browser, chromium, expect, type Locator, type Page, test, type WebSocketRoute } from '@playwright/test';

import { insecureOriginProxy, remoteOrigin } from './insecure-origin';
import { realTicketServer } from './real-ticket-server';
import { installTerminalFixture, project } from './terminal-feedback-fixture';

// HS2-1YX5VS: a desktop with the terminal in its bottom drawer and a phone looking at the same
// terminal fought over the PTY size, redrawing it about once a second. Like many TUIs, this
// synthetic program asks for the cursor position (DSR, `ESC[6n`) after every resize, and every
// attached xterm answers it. Each answer counted as a keystroke, so the idle viewer took the size
// back, the resize made the program ask again, and so on. It prints each size it receives so a
// failure shows the churn, and echoes each line after the replies the terminals sent before it.
const SIZE_REPORTER = String.raw`import os,signal,sys,termios
attrs = termios.tcgetattr(0)
attrs[3] &= ~termios.ECHO
termios.tcsetattr(0, termios.TCSANOW, attrs)
def report(*_):
    c,r = os.get_terminal_size(0)
    sys.stdout.write(f'\r\nSIZE {c}x{r}\r\n\x1b[6n')
    sys.stdout.flush()
signal.signal(signal.SIGWINCH, report)
report()
while True:
    line = sys.stdin.readline()
    if not line:
        break
    sys.stdout.write('ECHO:' + line.replace('\x1b', '^['))
    sys.stdout.flush()
`;

// HS2-0GZZHV: every attached xterm answered the program's cursor-position query, so the PTY read one
// reply per viewer. Only the viewer the server names (the driver) answers now.
/** How many cursor-position replies (`ESC[row;colR`) one terminal input message carries. */
function cursorPositionReplies(text: string): number {
  return text
    .split('\u001b[')
    .slice(1)
    .filter((sequence) => /^\d+;\d+R/.test(sequence)).length;
}

type SizeFrame = { cols: number; rows: number; drivenBy: string | null; at: number };
type Viewer = {
  page: Page;
  /** Every applied-size frame the server sent this page. */
  sizes: SizeFrame[];
  /** When each cursor-position reply (`ESC[r;cR`) this page sent toward the PTY left (HS2-0GZZHV). */
  replies: number[];
  /** The page-side ends of this page's attach sockets, newest last. */
  attaches: WebSocketRoute[];
  close: () => Promise<void>;
};

async function openViewer(
  browser: Browser,
  baseURL: string,
  server: Awaited<ReturnType<typeof realTicketServer>>,
  options: { width: number; height: number; mobile: boolean },
): Promise<Viewer> {
  const proxy = await insecureOriginProxy(baseURL, server),
    context = await browser.newContext({
      proxy: { server: proxy.url },
      viewport: { width: options.width, height: options.height },
      isMobile: options.mobile,
      hasTouch: options.mobile,
    }),
    page = await context.newPage();
  const viewer: Viewer = {
    page,
    sizes: [],
    replies: [],
    attaches: [],
    close: async () => {
      await page.unrouteAll({ behavior: 'ignoreErrors' });
      await context.close();
      await proxy.close();
    },
  };
  await page.addInitScript((root) => {
    localStorage.setItem('hotsheet.open-projects', JSON.stringify([root]));
  }, project.root);
  await installTerminalFixture(page, false, false, { nativeSocket: true, warmLimit: 0 });
  await page.route(/\/__hotsheet\/(?:remote\/)?projects\/open$/, (route) =>
    route.fulfill({ status: 201, json: { ...project, root: server.store } }),
  );
  await page.route('**/__hotsheet/project-api/terminal-feedback/terminals**', async (route) => {
    const url = new URL(route.request().url()),
      path = url.pathname.replace(project.apiPath, '');
    const response = await route.fetch({
      url: `${server.url}${path}${url.search}`,
      headers: { ...route.request().headers(), 'X-Hotsheet-Secret': server.secret },
    });
    await route.fulfill({ response });
  });
  await page.routeWebSocket(/\/terminals\/[^/?]+\/attach(?:\?.*)?$/, (route) => {
    viewer.attaches.push(route);
    const upstream = route.connectToServer();
    route.onMessage((message) => {
      const text = typeof message === 'string' ? message : message.toString('latin1');
      for (let reply = cursorPositionReplies(text); reply > 0; reply -= 1) viewer.replies.push(Date.now());
      upstream.send(message);
    });
    upstream.onMessage((message) => {
      if (typeof message === 'string' && message.includes('"pty_size"')) {
        const frame = JSON.parse(message) as { pty_size: { cols: number; rows: number }; driven_by?: string | null };
        viewer.sizes.push({ ...frame.pty_size, drivenBy: frame.driven_by ?? null, at: Date.now() });
      }
      route.send(message);
    });
  });
  await page.routeWebSocket(/\/ws\/sync(?:\?.*)?$/, (route) => {
    route.connectToServer();
  });
  await page.goto(remoteOrigin);
  return viewer;
}

/** The desktop opens the session in its bottom drawer, as in the maintainer's report. */
async function openDesktopDrawer(page: Page): Promise<Locator> {
  await page.getByRole('button', { name: 'Workspace Grid', exact: true }).click();
  const dashboard = page.getByRole('region', { name: 'Workspace Grid' });
  await dashboard.locator('[data-terminal-key="terminal-feedback:live"]').click();
  await dashboard
    .getByRole('dialog', { name: /^Magnified live$/i })
    .getByRole('button', { name: /^Open live in project terminal drawer$/i })
    .click();
  const drawer = page.locator('[data-component="terminal-session"] [data-terminal-id="live"]');
  await expect(drawer).toHaveAttribute('data-connection', 'connected');
  return drawer;
}

/** The phone looks at the same session: its Workspace Grid, then the tile tapped open. */
async function openPhoneTerminal(page: Page): Promise<Locator> {
  await page.getByRole('button', { name: 'Workspace Grid', exact: true }).click();
  const tile = page
    .getByRole('region', { name: 'Workspace Grid' })
    .locator('[data-terminal-key="terminal-feedback:live"]')
    .first();
  await expect(tile.locator('[data-component="terminal-viewport"]').first()).toHaveAttribute(
    'data-connection',
    'connected',
  );
  await tile.tap();
  const viewport = page
    .getByRole('dialog', { name: /^Magnified live$/i })
    .locator('[data-component="terminal-viewport"]');
  await expect(viewport).toHaveAttribute('data-connection', 'connected');
  return viewport;
}

/** How many cursor-position replies each viewer sent toward the PTY from `from` on. */
function repliesSince(devices: { desktop: Viewer; phone: Viewer }, from: number) {
  const count = (viewer: Viewer) => viewer.replies.filter((at) => at >= from).length;
  return { desktop: count(devices.desktop), phone: count(devices.phone) };
}

/** The sizes this viewer received from `from` on, after `quietMs` more of waiting. */
async function sizesAfter(viewer: Viewer, from: number, quietMs: number) {
  await viewer.page.waitForTimeout(quietMs);
  return viewer.sizes.filter((size) => size.at >= from).map(({ cols, rows }) => `${cols}x${rows}`);
}

test('the last device a human used keeps the terminal size; idle viewers never fight (HS2-1YX5VS, HS2-0GZZHV)', async ({
  baseURL,
}, testInfo) => {
  test.setTimeout(180_000);
  const server = await realTicketServer(),
    browser = await chromium.launch();
  const viewers: Viewer[] = [];
  try {
    await server.request('/terminals', 'POST', {
      id: 'live',
      command: 'python3',
      args: ['-u', '-c', SIZE_REPORTER],
      cwd: server.store,
    });
    const desktop = await openViewer(browser, baseURL!, server, { width: 1440, height: 900, mobile: false });
    viewers.push(desktop);
    const drawer = await openDesktopDrawer(desktop.page);
    await drawer.locator('.xterm-helper-textarea').focus();
    // Focusing the drawer takes the size; the program's query after that resize is answered
    // before typing, so its reply cannot land inside the typed word.
    await expect(drawer).toHaveAttribute('data-driving', 'true');
    const desktopGrid = (await drawer.getAttribute('data-grid-size'))!;
    await expect(drawer).toHaveAttribute('data-pty-size', desktopGrid);
    await expect.poll(() => desktop.replies.length).toBeGreaterThan(0);
    await desktop.page.keyboard.type('desk\n');
    await expect(drawer.locator('.xterm-rows')).toContainText(/ECHO:\S*desk/);

    // Opening and typing in the terminal on the phone is that device's interaction: it takes the size.
    let from = Date.now();
    const phone = await openViewer(browser, baseURL!, server, { width: 390, height: 844, mobile: true });
    viewers.push(phone);
    const phoneTerminal = await openPhoneTerminal(phone.page);
    await phoneTerminal.locator('.xterm-helper-textarea').focus();
    await phone.page.keyboard.type('phone\n');
    await expect(phoneTerminal.locator('.xterm-rows')).toContainText(/ECHO:\S*phone/);
    const phoneGrid = (await phoneTerminal.getAttribute('data-grid-size'))!;
    expect(phoneGrid).not.toBe(desktopGrid);
    await expect(drawer).toHaveAttribute('data-pty-size', phoneGrid);
    await expect(phoneTerminal).toHaveAttribute('data-driving', 'true');
    await expect(drawer).toHaveAttribute('data-driving', 'false');
    await expect(phoneTerminal).toHaveAttribute('data-answers-queries', 'true');
    await expect(drawer).toHaveAttribute('data-answers-queries', 'false');
    // The phone's takeover resized the PTY once, so the program asked once: only the phone answered,
    // even though the desktop drawer, the desktop grid tile, and the phone grid tile all saw the query.
    const devices = { desktop, phone };
    await expect.poll(() => repliesSince(devices, from)).toEqual({ desktop: 0, phone: 1 });

    // Both viewers sit idle across two heartbeats. Before the fix the size flipped about every 0.5 s.
    from = Date.now();
    expect(await sizesAfter(desktop, from, 12_000)).toEqual([]);
    expect(phone.sizes.filter((size) => size.at >= from)).toEqual([]);
    expect(repliesSince(devices, from)).toEqual({ desktop: 0, phone: 0 });
    await phone.page.screenshot({ path: testInfo.outputPath('hs2-1yx5vs-phone-authority.png') });
    await desktop.page.screenshot({ path: testInfo.outputPath('hs2-1yx5vs-desktop-passive.png') });

    // Typing on the desktop hands the size back exactly once, and it stays there.
    from = Date.now();
    await drawer.locator('.xterm-helper-textarea').focus();
    await desktop.page.keyboard.type('back\n');
    await expect(drawer).toHaveAttribute('data-pty-size', desktopGrid);
    expect(await sizesAfter(desktop, from, 8_000)).toEqual([desktopGrid]);
    // The new driver answers the resize's query; the old one stopped.
    expect(repliesSince(devices, from)).toEqual({ desktop: 1, phone: 0 });

    // Typing on the phone takes it again, once.
    from = Date.now();
    await phoneTerminal.locator('.xterm-helper-textarea').focus();
    await phone.page.keyboard.type('again\n');
    await expect(drawer).toHaveAttribute('data-pty-size', phoneGrid);
    expect(await sizesAfter(desktop, from, 8_000)).toEqual([phoneGrid]);
    expect(repliesSince(devices, from)).toEqual({ desktop: 0, phone: 1 });

    // The passive desktop reconnecting does not take the size from the phone.
    from = Date.now();
    const desktopAttaches = desktop.attaches.length;
    for (const attach of desktop.attaches) await attach.close();
    await expect.poll(() => desktop.attaches.length, { timeout: 30_000 }).toBeGreaterThan(desktopAttaches);
    await expect(drawer).toHaveAttribute('data-connection', 'connected');
    expect(await sizesAfter(phone, from, 8_000)).toEqual([]);
    await expect(drawer).toHaveAttribute('data-pty-size', phoneGrid);
    expect(repliesSince(devices, from)).toEqual({ desktop: 0, phone: 0 });

    // When the phone (the authority) goes away, the desktop takes over at once.
    from = Date.now();
    await phone.close();
    viewers.splice(viewers.indexOf(phone), 1);
    await expect(drawer).toHaveAttribute('data-pty-size', desktopGrid);
    await expect(drawer).toHaveAttribute('data-driving', 'true');
    expect(await sizesAfter(desktop, from, 3_000)).toEqual([desktopGrid]);
    // The lone desktop answers the query its takeover caused.
    await expect(drawer).toHaveAttribute('data-answers-queries', 'true');
    expect(repliesSince(devices, from).desktop).toBe(1);
    await desktop.page.screenshot({ path: testInfo.outputPath('hs2-1yx5vs-desktop-after-phone-left.png') });
  } finally {
    for (const viewer of viewers) await viewer.close();
    await browser.close();
    await server.stop();
  }
});
