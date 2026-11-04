import { expect, type Page, test, type WebSocketRoute } from '@playwright/test';

import { ticketsJson } from './ticket-page-fixture';

/**
 * HS2-KPJWBK: a long-lived page learns from the host's change bridge which client build it serves.
 * The routed bridge socket plays the real host contract: every subscription answer carries
 * `client_build`, and a host restart drops the socket so the page reconnects and hears it again.
 */
const MUX = '/__hotsheet/change-mux/poll';
const WS_MUX = '/__hotsheet/change-mux/ws';
const META = 'meta[name="hotsheet-client-build"]';
const project = {
  id: 'bundle-project',
  root: '/work/bundle',
  name: 'Bundle',
  stores: ['/work/bundle.hs2'],
  apiPath: '/__hotsheet/project-api/bundle-project',
  changeMuxPath: MUX,
  compatibility: { kind: 'compatible', revisionMismatch: false, sourceStale: false, canRestartServer: false },
};

async function installHost(page: Page) {
  const host = {
    served: undefined as string | undefined,
    socket: undefined as WebSocketRoute | undefined,
    sockets: 0,
    answers: 0,
    /** The bundle id the next document load is stamped with; undefined keeps the dev server's id. */
    documentBuild: undefined as string | undefined,
    requests: [] as string[],
  };
  await page.routeWebSocket(/.*/, (socket) => {
    if (new URL(socket.url()).pathname !== WS_MUX) {
      void socket.close();
      return;
    }
    host.socket = socket;
    host.sockets += 1;
    socket.onMessage((raw) => {
      const command = JSON.parse(String(raw)) as {
        type: string;
        client: string;
        project: string;
        subscription: number;
      };
      if (command.type !== 'subscribe') return;
      host.answers += 1;
      socket.send(
        JSON.stringify({
          version: 2,
          type: 'subscribed',
          client: command.client,
          project: command.project,
          subscription: command.subscription,
          ...(host.served ? { client_build: host.served } : {}),
        }),
      );
    });
  });
  page.on('request', (request) => {
    const path = new URL(request.url()).pathname;
    if (path.startsWith('/__hotsheet/')) host.requests.push(`${request.method()} ${path}`);
  });
  await page.route('**/*', async (route) => {
    const request = route.request(),
      path = new URL(request.url()).pathname;
    if (request.resourceType() === 'document' && host.documentBuild) {
      // The "new bundle" a restarted host serves: the same document, stamped with its build id.
      const response = await route.fetch();
      const html = (await response.text()).replace(
        /(<meta name="hotsheet-client-build" content=")[^"]*(")/u,
        `$1${host.documentBuild}$2`,
      );
      return route.fulfill({ response, body: html });
    }
    if (path === '/__hotsheet/projects/open') return route.fulfill({ status: 201, json: project });
    if (path === MUX) {
      // A genuine long poll: idle, it stays pending until its bounded timeout.
      const polls = (request.postDataJSON() as { polls: { project: string; since?: number; timeout_ms: number }[] })
        .polls;
      if (polls.every((poll) => poll.since !== undefined && poll.timeout_ms > 0))
        await new Promise((resolve) => setTimeout(resolve, Math.min(2_000, ...polls.map((poll) => poll.timeout_ms))));
      return route
        .fulfill({
          json: {
            version: 1,
            results: polls.map((poll) => ({
              project: poll.project,
              status: 200,
              body: { cursor: 1, events: [], overflow: false },
            })),
          },
        })
        .catch(() => undefined);
    }
    if (path.endsWith('/ws/poll')) return route.fulfill({ json: { cursor: 1, events: [], overflow: false } });
    if (path.endsWith('/providers')) return route.fulfill({ json: [] });
    if (path.endsWith('/tickets')) return route.fulfill({ json: ticketsJson(request, []) });
    if (path.endsWith('/repository/status'))
      return route.fulfill({ json: { ahead: 0, behind: 0, staged: 0, unstaged: 0, untracked: 0, conflicted: 0 } });
    // The project's other collections are empty; nothing reaches the dev bridge, which has no session.
    if (
      [
        '/corrupt-tickets',
        '/commands',
        '/command-groups',
        '/command-runs',
        '/views',
        '/terminals',
        '/permissions',
        '/connections',
        '/drive/sessions',
        '/notifications',
        '/migration-jobs',
      ].some((suffix) => path.endsWith(suffix))
    )
      return route.fulfill({ json: [] });
    if (path.startsWith('/__hotsheet/'))
      return route.fulfill({ status: 404, json: { error: `Unhandled ${request.method()} ${path}` } });
    return route.continue();
  });
  return host;
}

async function openProject(page: Page) {
  await page.goto('/');
  await page.getByRole('button', { name: 'Open Project' }).click();
  await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();
  await expect(page.getByRole('heading', { name: 'Queue', level: 1 })).toBeVisible();
}

const loadedBuild = (page: Page) => page.locator(META).getAttribute('content');

test('a host restart onto a newer client raises the notice, and Reload loads the new bundle (HS2-KPJWBK)', async ({
  page,
}) => {
  test.setTimeout(120_000);
  await page.setViewportSize({ width: 1440, height: 900 });
  const host = await installHost(page);
  await page.goto('/');
  const loaded = await loadedBuild(page);
  expect(loaded).toMatch(/^dev-/u);
  host.served = loaded!;
  await openProject(page);
  const notice = page.locator('[data-component="client-update-notice-banner"]');
  await expect.poll(() => host.answers).toBeGreaterThan(0);

  // A reconnect to a host that still serves this page's bundle raises nothing.
  const answers = host.answers;
  await host.socket!.close();
  await expect.poll(() => host.answers, { timeout: 30_000 }).toBeGreaterThan(answers);
  await expect(notice).toHaveCount(0);

  // The host restarts onto a newer bundle: the reconnect announces it.
  host.served = 'bundle-0123456789abcdef';
  await host.socket!.close();
  await expect(notice).toBeVisible({ timeout: 30_000 });
  await expect(notice).toContainText('A newer version of Hot Sheet is available');
  await expect(notice).toHaveAttribute('data-tone', 'info');
  // Wide, medium, narrow, then back to wide: the strip fits and keeps both actions reachable.
  for (const [index, width] of [1440, 1024, 390, 1440].entries()) {
    await page.setViewportSize({ width, height: width === 390 ? 844 : 900 });
    await expect(notice.getByRole('button', { name: 'Reload' })).toBeInViewport();
    await expect(notice.getByRole('button', { name: 'Dismiss' })).toBeInViewport();
    expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(width);
    await page.screenshot({
      path: `target/visual-captures/hs2-kpjwbk-client-update-notice-${index === 3 ? 'restored-' : ''}${width}.png`,
      animations: 'disabled',
    });
  }
  // Nothing reloads on its own.
  expect(await loadedBuild(page)).toBe(loaded);

  // Reload goes through the flush path and loads the bundle the host now serves.
  host.documentBuild = host.served;
  const answersBeforeReload = host.answers;
  const navigation = page.waitForEvent('framenavigated', (frame) => frame === page.mainFrame());
  await notice.getByRole('button', { name: 'Reload' }).click();
  await navigation;
  await expect(page.getByRole('heading', { name: 'Queue', level: 1 })).toBeVisible();
  expect(await loadedBuild(page)).toBe('bundle-0123456789abcdef');
  // The reloaded page subscribes again and hears the build it now runs: no notice.
  await expect.poll(() => host.answers, { timeout: 30_000 }).toBeGreaterThan(answersBeforeReload);
  await expect(notice).toHaveCount(0);
});

test('dismissal holds through same-build reconnects and idle or visibility changes add no requests (HS2-KPJWBK)', async ({
  page,
}) => {
  test.setTimeout(120_000);
  await page.setViewportSize({ width: 1440, height: 900 });
  const host = await installHost(page);
  host.served = 'bundle-fedcba9876543210';
  await openProject(page);
  const notice = page.locator('[data-component="client-update-notice-banner"]');
  await expect(notice).toBeVisible();
  await notice.getByRole('button', { name: 'Dismiss' }).click();
  await expect(notice).toHaveCount(0);
  // The same pair on a reconnect stays dismissed.
  const answers = host.answers;
  await host.socket!.close();
  await expect.poll(() => host.answers, { timeout: 30_000 }).toBeGreaterThan(answers);
  await expect(notice).toHaveCount(0);

  // Idle, and across hide/show, the notice issues no bridge requests and opens no new socket.
  await page.waitForTimeout(500);
  const before = host.requests.length,
    sockets = host.sockets;
  await page.evaluate(() => {
    for (const state of ['hidden', 'visible'] as const) {
      Object.defineProperty(document, 'visibilityState', { value: state, configurable: true });
      document.dispatchEvent(new Event('visibilitychange'));
    }
  });
  // Idle observation window; the assertion is that nothing happens in it.
  await page.waitForTimeout(3_000);
  expect(host.requests.slice(before)).toEqual([]);
  expect(host.sockets).toBe(sockets);

  // A later host build announces again despite the earlier dismissal.
  host.served = 'bundle-1111111111111111';
  await host.socket!.close();
  await expect(notice).toBeVisible({ timeout: 30_000 });
});
