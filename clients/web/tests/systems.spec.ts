import { mkdir, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';

import { expect, type Page, type Request, test } from '@playwright/test';

import { proxyFetch, realTicketServer } from './real-ticket-server';

/**
 * HS2-DR0ZSQ: the Systems screen in the shipped app against a real server. One real server plays this
 * computer (a local project tab) and a paired remote (a remote tab restored from the desktop's remote
 * list, its checkout-scoped paths rewritten to the native checkout as the bridge does). A second paired
 * remote has no open tab. Every reading is a push or a long poll; leaving the screen stops them.
 * Synthetic names and ids only.
 */
type Server = Awaited<ReturnType<typeof realTicketServer>>;
const BINDING = '01ARZ3NDEKTSV4RRFFQ69G5FAV';
const OFFLINE = '01ARZ3NDEKTSV4RRFFQ69G5FB0';

async function openMachines(page: Page, server: Server) {
  const localId = server.checkoutId;
  const apiPath = `/__hotsheet/project-api/${localId}`;
  const remoteId = `remote:${BINDING}:${server.checkoutId}`;
  const remotePath = `/__hotsheet/project-api/${encodeURIComponent(remoteId)}`;
  // Change streams fall back to their long poll through each tab's path.
  await page.routeWebSocket(/.*/u, (route) => {
    void route.close();
  });
  await page.route(/\/__hotsheet\/(?:remote\/)?projects\/open$/u, (route) =>
    route.fulfill({
      status: 201,
      json: { id: localId, root: server.root, name: 'Launch', stores: [server.store], apiPath },
    }),
  );
  await page.route(/\/__hotsheet\/remotes(?:\/.*)?$/u, (route) => {
    const path = new URL(route.request().url()).pathname.replace('/__hotsheet/remotes', '');
    const remote = (id: string, origin: string) => ({
      id,
      label: `Pairing ${id.slice(-4)}`,
      origin,
      identity: { server_id: `server-${id.slice(-4)}`, ca_fingerprint: null },
      revision: 1,
      revoked: false,
      last_failure: null,
      stale: false,
      last_confirmed_checkouts: [{ id: server.checkoutId, name: 'API' }],
    });
    if (path === '')
      return route.fulfill({
        json: [remote(BINDING, 'https://build.example.test:7400'), remote(OFFLINE, 'https://travel.example.test')],
      });
    if (path === '/selected-projects')
      return route.fulfill({ json: [{ binding_id: BINDING, checkout_id: server.checkoutId }] });
    if (path === `/${BINDING}/projects/${server.checkoutId}/open`)
      return route.fulfill({
        json: {
          id: remoteId,
          root: remoteId,
          name: 'Build Host · API',
          remoteHost: 'Build Host',
          stores: [],
          apiPath: remotePath,
        },
      });
    if (path.endsWith('/session')) return route.fulfill({ json: { binding_id: path.split('/')[1] } });
    return route.fulfill({ status: 404, json: { error: 'Not in this fixture.' } });
  });
  for (const [prefix, encoded] of [
    [apiPath, undefined],
    [remotePath, encodeURIComponent(remoteId)],
  ] as const)
    await page.route(`**${prefix}/**`, async (route) => {
      const incoming = new URL(route.request().url());
      let path = incoming.pathname.slice(prefix.length);
      if (encoded) path = path.replace(`/checkouts/${encoded}`, `/checkouts/${server.checkoutId}`);
      try {
        const response = await proxyFetch(route, {
          url: `${server.url}${path}${incoming.search}`,
          headers: { ...route.request().headers(), 'X-Hotsheet-Secret': server.secret },
          timeout: 60_000,
        });
        if (encoded && path.endsWith('/target-capacity') && response.ok()) {
          await route.fulfill({ response, json: remoteReportsSignIn(await response.json()) });
          return;
        }
        await route.fulfill({ response });
      } catch {
        await route.abort().catch(() => undefined);
      }
    });
  await page.addInitScript((root) => {
    localStorage.setItem('hotsheet.open-projects', JSON.stringify([root]));
  }, server.root);
  return { apiPath, remotePath, remoteId };
}

/**
 * HS2-S1FEYY: the fixture host runs with AI tool probes off, so it reports no sign-in state. The
 * remote plays a host that does: Claude signed in, Codex signed out, OpenCode not installed.
 */
function remoteReportsSignIn(snapshot: Record<string, unknown>) {
  const capabilities = (snapshot.capabilities ?? { os: 'linux', arch: 'x86_64' }) as Record<string, unknown>;
  return {
    ...snapshot,
    tools: [
      { tool: 'claude', installed: true, configured: true },
      { tool: 'codex', installed: true, configured: true },
      { tool: 'opencode', installed: false, configured: true },
    ],
    capabilities: {
      ...capabilities,
      ai_tools: [
        { tool: 'claude', version: '2.1.0', signed_in: true },
        { tool: 'codex', version: '0.40.0', signed_in: false },
      ],
    },
  };
}

/** Every Systems read the page makes, by kind and tab path. */
function recordReads(page: Page, paths: { apiPath: string; remotePath: string }) {
  const reads: {
    kind: 'capacity' | 'cpu' | 'tools' | 'admission';
    machine: 'local' | 'remote';
    after: string | null;
  }[] = [];
  page.on('request', (request: Request) => {
    const url = new URL(request.url());
    const machine = url.pathname.startsWith(paths.remotePath)
      ? 'remote'
      : url.pathname.startsWith(paths.apiPath)
        ? 'local'
        : undefined;
    if (!machine) return;
    const kind = url.pathname.endsWith('/target-capacity')
      ? 'capacity'
      : url.pathname.endsWith('/host-health/cpu')
        ? 'cpu'
        : url.pathname.endsWith('/orchestration/tool-capacity')
          ? 'tools'
          : url.pathname.endsWith('/admission')
            ? 'admission'
            : undefined;
    if (kind) reads.push({ kind, machine, after: url.searchParams.get('after') });
  });
  return reads;
}

const machineRow = (page: Page, id: string) =>
  page.locator(`[data-component="systems-sidebar"] [data-machine-id="${id}"]`);

async function capture(page: Page, name: string) {
  await page.waitForFunction(() =>
    document
      .getAnimations()
      .every((animation) => animation.playState !== 'running' || animation.effect?.getTiming().iterations === Infinity),
  );
  await page.screenshot({ path: `target/visual-captures/hs2-dr0zsq-${name}.png` });
}

test('HS2-DR0ZSQ Systems shows this computer and each paired remote with live readings', async ({ page }) => {
  test.setTimeout(120_000);
  const server = await realTicketServer();
  try {
    const paths = await openMachines(page, server);
    const reads = recordReads(page, paths);
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.goto('/');
    await expect(page.locator('[data-tab-kind="project"][data-location="remote"]')).toHaveCount(1, {
      timeout: 30_000,
    });
    // Nothing reads machine capacity before Systems shows.
    expect(reads.filter((read) => read.kind === 'capacity')).toEqual([]);

    // The strip's Systems button sits after Workspace Grid and before Cross-Project Stats.
    const dashboards = page.locator('[aria-label="Global Dashboards"]').first();
    const modes = await dashboards
      .locator('[data-shell-mode]')
      .evaluateAll((buttons) => buttons.map((button) => button.getAttribute('data-shell-mode')));
    expect(modes.indexOf('systems')).toBe(modes.indexOf('terminals') + 1);
    const systems = dashboards.getByRole('button', { name: 'Systems' });
    await systems.click();
    await expect(systems).toHaveAttribute('aria-pressed', 'true');

    // Left rail: this computer, the connected remote, and the remote without an open tab.
    await expect(page.locator('[data-component="systems-sidebar"] [data-machine-id]')).toHaveCount(3);
    await expect(machineRow(page, 'local')).toHaveAttribute('aria-current', 'page');
    await expect(machineRow(page, `remote:${BINDING}`)).toContainText('Build Host');
    await expect(machineRow(page, `remote:${OFFLINE}`)).toContainText('Not connected');
    await expect(machineRow(page, `remote:${OFFLINE}`)).toContainText('Offline');
    // Workspace: this computer's cards, then what it is, once its capacity stream answers.
    const view = page.locator('[data-component="systems-view"]');
    await expect(view).toHaveAttribute('data-machine-id', 'local');
    await expect(view.locator('[data-host-context]')).toContainText('This Computer', { timeout: 30_000 });
    await expect(view.locator('[data-card="cpu"]')).toBeVisible();
    await expect(view.locator('[data-systems-section="Capabilities"]')).toContainText('Memory', { timeout: 30_000 });
    await expect(machineRow(page, 'local')).toContainText(/This Computer · \d+ AI sessions?/u);
    // Right rail: the machine and its AI sessions.
    await expect(page.locator('[data-component="systems-inspector"]')).toContainText('AI Sessions Here');
    await expect(page.getByText('No AI session is running here.')).toBeVisible();

    // The capacity stream replays its cursor: each later read waits on the version it saw.
    await expect
      .poll(() => reads.some((read) => read.kind === 'capacity' && read.machine === 'local' && read.after !== null), {
        timeout: 30_000,
      })
      .toBe(true);
    // The live CPU feed starts for the selected machine (a fixture host without a per-core reader
    // answers `unsupported` once, and the feed stops there).
    await expect
      .poll(() => reads.some((read) => read.kind === 'cpu' && read.machine === 'local'), { timeout: 30_000 })
      .toBe(true);
    await capture(page, 'local-1440');

    // The remote reads through its own tab's path, with its own live CPU feed.
    await machineRow(page, `remote:${BINDING}`).click();
    await expect(view).toHaveAttribute('data-machine-id', `remote:${BINDING}`);
    await expect(view.locator('[data-host-context]')).toContainText('Paired Remote', { timeout: 30_000 });
    await expect(view.locator('[data-systems-section="Capabilities"]')).toContainText('Memory', { timeout: 30_000 });
    await expect(page.locator('[data-component="systems-inspector"]')).toHaveAttribute(
      'data-machine-id',
      `remote:${BINDING}`,
    );
    // HS2-S1FEYY: each AI tool's sign-in state as the remote reports it, never an account.
    const tool = (name: string) => view.locator(`[data-systems-ai-tool="${name}"]`);
    await expect(tool('claude')).toHaveAttribute('data-signed-in', 'true');
    await expect(tool('claude')).toContainText('Signed In');
    await expect(tool('codex')).toHaveAttribute('data-signed-in', 'false');
    await expect(tool('codex')).toContainText('Signed Out');
    await expect(tool('codex')).toContainText('not signed in');
    await expect(tool('opencode')).toHaveAttribute('data-installed', 'false');
    await expect(tool('opencode')).toContainText('Not Installed');
    await expect
      .poll(() => reads.some((read) => read.kind === 'cpu' && read.machine === 'remote'), { timeout: 30_000 })
      .toBe(true);
    await expect
      .poll(() => reads.some((read) => read.kind === 'capacity' && read.machine === 'remote' && read.after !== null), {
        timeout: 30_000,
      })
      .toBe(true);

    // A remote without an open tab says how to connect it.
    await machineRow(page, `remote:${OFFLINE}`).click();
    await expect(view).toContainText('travel.example.test Is Not Connected');
    await expect(page.locator('[data-component="systems-inspector"]')).toContainText(
      'Sessions show while the machine is connected.',
    );
    // Back to this computer, then away: every Systems read stops.
    await machineRow(page, 'local').click();
    await expect(view).toHaveAttribute('data-machine-id', 'local');
    await page.locator('[data-tab-kind="project"][data-location="local"]').first().click();
    await expect(view).toHaveCount(0);
    const settled = reads.length;
    // An observation window, not a wait for state: a stopped stream must issue nothing during it.
    await page.waitForTimeout(3_000);
    expect(reads.slice(settled).filter((read) => read.kind !== 'tools')).toEqual([]);
  } finally {
    await server.stop(page);
  }
});

/**
 * HS2-5H8R0N: this machine's own admission file (synthetic limits), as `orchestration admission set`
 * writes it, in the fixture server's isolated home.
 */
async function ownerLimits(home: string, admission: Record<string, number>) {
  await writeFile(resolve(home, 'orchestration-admission.json'), JSON.stringify({ schema_version: 1, admission }));
}

test('HS2-5H8R0N Systems shows admission for each project placed here and this machine’s placement limits, live', async ({
  page,
}) => {
  test.setTimeout(120_000);
  const server = await realTicketServer();
  try {
    // The local project orchestrates one project, Widgets, placed on this computer.
    const base = `/checkouts/${server.checkoutId}/orchestration`;
    await server.request(`${base}/project-type`, 'PUT', { project_type: 'orchestration', actor: { role: 'human' } });
    const widgetsRoot = join(dirname(server.root), 'widgets');
    await mkdir(widgetsRoot, { recursive: true });
    const widgets = await server.request<{ id: string }>('/checkouts', 'POST', {
      root: widgetsRoot,
      stores: [server.store],
    });
    await server.request(`${base}/projects`, 'POST', {
      destination: { kind: 'local', checkout_id: widgets.id },
      label: 'Widgets',
      actor: { role: 'human' },
    });
    await ownerLimits(server.home, { max_sessions: 3, min_free_ram_mb: 1024 });
    const paths = await openMachines(page, server);
    const reads = recordReads(page, paths);
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.goto('/');
    await expect(page.locator('[data-tab-kind="project"][data-location="remote"]')).toHaveCount(1, {
      timeout: 30_000,
    });
    await page.locator('[aria-label="Global Dashboards"]').first().getByRole('button', { name: 'Systems' }).click();
    const inspector = page.locator('[data-component="systems-inspector"]');
    await expect(inspector).toHaveAttribute('data-machine-id', 'local');
    // Admission by Project: Widgets with the real admission answer (the fixture host may lack every tool).
    const admission = inspector.getByRole('region', { name: 'Admission by Project' });
    await expect(admission.locator('[data-systems-admission]')).toHaveCount(1, { timeout: 30_000 });
    await expect(admission).toContainText('Widgets');
    await expect(admission).toContainText(/Can start \d+|Refused|No AI tool allowed here|Unknown/u, {
      timeout: 30_000,
    });
    // Placement Limits: the owner's two limits, the rest left to each project.
    const limits = inspector.getByRole('region', { name: 'Placement Limits' });
    const limit = (label: string) => limits.locator(`[data-systems-limit="${label}"]`);
    await expect(limit('Sessions per Machine')).toContainText('3', { timeout: 30_000 });
    await expect(limit('Free Memory Floor')).toContainText('1.0 GB');
    await expect(limit('CPU Ceiling')).toContainText('Set by each project');
    await expect(limit('Free Disk Floor')).toContainText('Set by each project');
    // The owner changes the limits: the capacity long poll answers with a new version, the rail
    // follows without a reload, and admission is asked again for the changed room.
    const asked = reads.filter((read) => read.kind === 'admission').length;
    await ownerLimits(server.home, { max_sessions: 0, max_cpu_percent: 80 });
    await expect(limit('Sessions per Machine')).toContainText('0', { timeout: 30_000 });
    await expect(limit('CPU Ceiling')).toContainText('80%');
    await expect(limit('Free Memory Floor')).toContainText('Set by each project');
    await expect
      .poll(() => reads.filter((read) => read.kind === 'admission').length, { timeout: 30_000 })
      .toBeGreaterThan(asked);
    // The paired remote (the same fixture server, reached through its tab) runs no Widgets site.
    await machineRow(page, `remote:${BINDING}`).click();
    await expect(inspector).toHaveAttribute('data-machine-id', `remote:${BINDING}`);
    await expect(admission).toContainText('No orchestrated project runs on this machine.');
    await expect(limit('Sessions per Machine')).toContainText('0', { timeout: 30_000 });
  } finally {
    await server.stop(page);
  }
});

test('HS2-DR0ZSQ an idle Systems screen reads only through long polls, at a bounded rate', async ({ page }) => {
  test.setTimeout(120_000);
  const server = await realTicketServer();
  try {
    const paths = await openMachines(page, server);
    const reads = recordReads(page, paths);
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.goto('/');
    await expect(page.locator('[data-tab-kind="project"][data-location="remote"]')).toHaveCount(1, {
      timeout: 30_000,
    });
    await page.locator('[aria-label="Global Dashboards"]').first().getByRole('button', { name: 'Systems' }).click();
    await expect(page.locator('[data-component="systems-view"] [data-systems-section="Capabilities"]')).toContainText(
      'Memory',
      { timeout: 30_000 },
    );
    // Settle: both capacity streams are holding a versioned read.
    await expect
      .poll(
        () =>
          ['local', 'remote'].every((machine) =>
            reads.some((read) => read.kind === 'capacity' && read.machine === machine && read.after !== null),
          ),
        { timeout: 30_000 },
      )
      .toBe(true);
    const start = reads.length;
    const windowMs = 16_000;
    // An idle window: nothing on the page changes, only the streams run.
    await page.waitForTimeout(windowMs);
    const idle = reads.slice(start);
    const count = (kind: string, machine: string) =>
      idle.filter((read) => read.kind === kind && read.machine === machine).length;
    // The capacity long poll holds up to eight seconds unless the machine's room changes; allow for
    // a change or two of a busy test machine, never a request per second.
    for (const machine of ['local', 'remote']) expect(count('capacity', machine)).toBeLessThanOrEqual(6);
    // Every capacity read in the window waits on the version it saw.
    expect(idle.filter((read) => read.kind === 'capacity' && read.after === null)).toEqual([]);
    // Only the selected machine has a live CPU feed, paced by the server's two-second frames.
    expect(count('cpu', 'remote')).toBe(0);
    expect(count('cpu', 'local')).toBeLessThanOrEqual(windowMs / 2_000 + 2);
    // AI Tool Capacity is read when the screen opens, never again while it stays open.
    expect(idle.filter((read) => read.kind === 'tools')).toEqual([]);
    // HS2-5H8R0N: with no orchestration project open, nothing asks admission.
    expect(idle.filter((read) => read.kind === 'admission')).toEqual([]);
  } finally {
    await server.stop(page);
  }
});

test('HS2-DR0ZSQ Systems keeps both rails and its layout from wide to phone and back, light and dark', async ({
  page,
}) => {
  test.setTimeout(180_000);
  // The real host sampler, so the cards show this machine's readings rather than "Unavailable".
  const server = await realTicketServer({ env: { HOTSHEET_HOST_HEALTH_SAMPLER: 'on' } });
  try {
    await openMachines(page, server);
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.goto('/');
    await expect(page.locator('[data-tab-kind="project"][data-location="remote"]')).toHaveCount(1, {
      timeout: 30_000,
    });
    await page.locator('[aria-label="Global Dashboards"]').first().getByRole('button', { name: 'Systems' }).click();
    const view = page.locator('[data-component="systems-view"]');
    await expect(view.locator('[data-card="disk"]')).not.toHaveAttribute('data-status', 'unavailable', {
      timeout: 30_000,
    });
    await expect(view.locator('[data-systems-section="Capabilities"]')).toContainText('Memory', { timeout: 30_000 });
    for (const scheme of ['light', 'dark'] as const) {
      await page.emulateMedia({ colorScheme: scheme });
      for (const [width, height] of [
        [1440, 900],
        [1100, 800],
        [390, 844],
        [1440, 900],
      ] as const) {
        await page.setViewportSize({ width, height });
        await expect(view).toBeVisible();
        // A phone hides both rails (overlays there, opened from the toolbar); wider layouts restore them.
        const rails = page.locator('[data-component="systems-sidebar"], [data-component="systems-inspector"]');
        if (width < 600) for (const rail of await rails.all()) await expect(rail).not.toBeInViewport();
        else for (const rail of await rails.all()) await expect(rail).toBeInViewport();
        // Nothing scrolls the page sideways at any width.
        expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
        await capture(page, `local-${String(width)}-${scheme}`);
      }
    }
    await page.emulateMedia({ colorScheme: 'light' });
    await page.setViewportSize({ width: 1440, height: 900 });
    await machineRow(page, `remote:${BINDING}`).click();
    await expect(view.locator('[data-host-context]')).toContainText('Paired Remote', { timeout: 30_000 });
    await expect(view.locator('[data-systems-section="Capabilities"]')).toContainText('Memory', { timeout: 30_000 });
    await capture(page, 'remote-1440-light');
    await page.emulateMedia({ colorScheme: 'dark' });
    await capture(page, 'remote-1440-dark');
  } finally {
    await server.stop(page);
  }
});
