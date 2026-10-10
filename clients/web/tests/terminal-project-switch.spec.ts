import { expect, type Page, test } from '@playwright/test';

// HS2-WGTQ6X: switching back to a project keeps its drawer terminal alive — the same emulator
// and socket, painted on the first frame — instead of rebuilding it and replaying scrollback.

const projects = ['a', 'b'].map((id) => ({
  id,
  root: `/work/${id}`,
  name: `Project ${id.toUpperCase()}`,
  stores: [`/work/${id}.hs2`],
  apiPath: `/__hotsheet/project-api/${id}`,
  needsTicketSetup: false,
  needsHs1Migration: false,
}));
const capabilities = {
  create: true,
  update: true,
  close: true,
  notes: true,
  up_next: true,
  claims: true,
  query_fields: [],
};

type SocketState = { url: string; readyState: number; closed: boolean };
type SwitchWindow = typeof window & {
  __switchSockets: SocketState[];
  __frames?: Array<{ project?: string; tagged: boolean; text: string }>;
};

async function installWorkspace(page: Page) {
  await page.addInitScript(
    ({ roots, ids }) => {
      localStorage.setItem('hotsheet.open-projects', JSON.stringify(roots));
      localStorage.setItem('hotsheet.workspace.active-project-root.v1', roots[0]);
      localStorage.setItem('hotsheet.terminals.drawer-open', 'true');
      for (const id of ids) localStorage.setItem(`hotsheet.project.${id}.terminal-drawer-selection`, 'shell');
      const sockets: SocketState[] = [];
      class FakeSocket extends EventTarget {
        static CONNECTING = 0;
        static OPEN = 1;
        static CLOSING = 2;
        static CLOSED = 3;
        readyState = 0;
        binaryType = 'blob';
        state: SocketState;
        constructor(public url: string) {
          super();
          this.state = { url, readyState: 0, closed: false };
          sockets.push(this.state);
          setTimeout(() => {
            if (this.readyState === FakeSocket.CLOSED) return;
            this.readyState = this.state.readyState = FakeSocket.OPEN;
            this.dispatchEvent(new Event('open'));
            const project = /project-api\/([^/]+)\//.exec(url)?.[1] ?? '?';
            this.dispatchEvent(
              new MessageEvent('message', {
                data: new TextEncoder().encode(`project ${project} shell ready\r\n${project}$ `).buffer,
              }),
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
          this.readyState = this.state.readyState = FakeSocket.CLOSED;
          this.state.closed = true;
          this.dispatchEvent(new CloseEvent('close'));
        }
      }
      Object.assign(window, { WebSocket: FakeSocket, __switchSockets: sockets });
    },
    { roots: projects.map((item) => item.root), ids: projects.map((item) => item.id) },
  );
  await page.route('**/*', async (route) => {
    const request = route.request(),
      path = decodeURIComponent(new URL(request.url()).pathname);
    if (path === '/__hotsheet/projects/open' && request.method() === 'POST') {
      const root = (request.postDataJSON() as { root: string }).root;
      return route.fulfill({ status: 201, json: projects.find((item) => item.root === root) ?? projects[0] });
    }
    const api = /\/__hotsheet\/project-api\/([^/]+)(\/.*)$/.exec(path);
    if (!api) {
      if (path.startsWith('/__hotsheet/')) return route.fulfill({ json: [] });
      return route.continue();
    }
    const [, id, rest] = api,
      project = projects.find((item) => item.id === id)!;
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

const tab = (page: Page, id: string) =>
  page.locator(`[data-tab-kind="project"][data-project-id="${id}"]`).getByRole('tab');
const narrow = (page: Page) => page.viewportSize()!.width <= 1024;
// Wide layouts switch projects with tabs; phones use the project Select.
async function switchTo(page: Page, id: string) {
  if (!narrow(page)) return tab(page, id).click();
  const select = page.locator('wa-select[name="mobile-project"]');
  await select.click();
  await select.locator(`wa-option[value="${id}"]`).click();
}
async function expectActive(page: Page, id: string) {
  if (narrow(page)) await expect(page.locator('wa-select[name="mobile-project"]')).toHaveJSProperty('value', id);
  else await expect(tab(page, id)).toHaveAttribute('aria-selected', 'true');
}
const dedicated = (page: Page, id: string) =>
  page.locator(
    `[data-component="terminal-drawer"] [data-component="terminal-viewport"][data-project-id="${id}"][data-terminal-id="shell"]`,
  );
const attachSockets = (page: Page, id: string) =>
  page.evaluate(
    (project) =>
      (window as SwitchWindow).__switchSockets.filter((socket) =>
        socket.url.includes(`/project-api/${project}/terminals/shell/attach`),
      ),
    id,
  );

for (const [width, height] of [
  [1440, 900],
  [390, 844],
] as const)
  test(`returning to a project shows its live drawer terminal on the first frame at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height });
    await installWorkspace(page);
    await page.goto('/?dev-review=false');
    await expectActive(page, 'a');
    await expect(dedicated(page, 'a')).toHaveAttribute('data-connection', 'connected');
    await expect(dedicated(page, 'a')).toHaveAttribute('data-geometry-ready', 'true');
    await dedicated(page, 'a').evaluate((element) => {
      (element as HTMLElement & { __switchTag?: string }).__switchTag = 'a';
    });

    await switchTo(page, 'b');
    await expect(dedicated(page, 'b')).toHaveAttribute('data-connection', 'connected');
    await expect(dedicated(page, 'a')).toHaveCount(0);
    // A's viewport is parked out of the page with its socket still open.
    const parked = page.locator('[data-terminal-viewport-parking] [data-project-id="a"][data-parked="true"]');
    await expect(parked).toHaveCount(1);
    await expect(parked).toHaveAttribute('data-renderer', 'dom');
    expect(await attachSockets(page, 'a')).toEqual([expect.objectContaining({ readyState: 1, closed: false })]);

    // Sample every frame from the click on: the first frame showing project A must already show
    // the same live terminal with its content.
    await page.evaluate(() => {
      const state = window as SwitchWindow;
      state.__frames = [];
      const sample = () => {
        const viewport = document.querySelector<HTMLElement & { __switchTag?: string }>(
          '[data-component="terminal-drawer"] [data-component="terminal-viewport"]:not([data-parked="true"])',
        );
        state.__frames!.push({
          project: viewport?.dataset.projectId,
          tagged: viewport?.__switchTag === 'a',
          text: viewport?.querySelector('.xterm-rows')?.textContent ?? '',
        });
        if (state.__frames!.length < 30) requestAnimationFrame(sample);
      };
      requestAnimationFrame(sample);
    });
    await switchTo(page, 'a');
    await expectActive(page, 'a');
    await expect.poll(() => page.evaluate(() => (window as SwitchWindow).__frames!.length)).toBe(30);
    const frames = await page.evaluate(() => (window as SwitchWindow).__frames!);
    const first = frames.find((frame) => frame.project === 'a');
    expect(first, JSON.stringify(frames)).toMatchObject({ tagged: true });
    expect(first!.text).toContain('project a shell ready');
    await page.screenshot({ path: `test-results/hs2-wgtq6x-return-${width}.png` });

    // Same socket, no replay; B is parked warm in turn.
    expect(await attachSockets(page, 'a')).toHaveLength(1);
    expect(await attachSockets(page, 'b')).toEqual([expect.objectContaining({ readyState: 1, closed: false })]);
    await expect(dedicated(page, 'a')).toHaveAttribute('data-connection', 'connected');
    await expect(dedicated(page, 'a')).not.toHaveAttribute('data-parked', 'true');
    if (width > 1024) await expect(dedicated(page, 'a')).toHaveAttribute('data-renderer', 'dom');

    // Typing still reaches the kept-alive terminal after the round trip.
    await dedicated(page, 'a').click();
    await expect(dedicated(page, 'a')).toHaveAttribute('data-viewport-visible', 'true');

    // Closing the other project evicts its parked terminal and closes the socket.
    if (width <= 1024) return; // phones switch projects through a picker without tab close buttons
    await tab(page, 'b').hover();
    await page.getByRole('button', { name: 'Close Project B' }).click();
    const dialog = page.locator('[data-component="project-close-dialog"]');
    await expect(dialog).toHaveJSProperty('open', true);
    await dialog
      .getByRole('button', { name: /^(Close Project|Keep Running)$/ })
      .first()
      .click();
    await expect(tab(page, 'b')).toHaveCount(0);
    await expect.poll(async () => (await attachSockets(page, 'b')).every((socket) => socket.closed)).toBe(true);
    await expect(page.locator('[data-terminal-viewport-parking] [data-project-id="b"]')).toHaveCount(0);
    await expect(dedicated(page, 'a')).toHaveAttribute('data-connection', 'connected');
  });
