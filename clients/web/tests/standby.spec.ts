import { expect, type Page, type WebSocketRoute } from '@playwright/test';
import { test } from '@playwright/test';

import { openDemoProject } from './mobile-project-fixture';

const MINUTE = 60_000;

/** One store's live snapshot in the server's exact shape (`GET /live-state`); synthetic content only. */
function store(
  id: string,
  name: string,
  checkout: string,
  word: 'active' | 'stalled' | 'stopped',
  now: number,
  revision = 1,
) {
  const minute = Math.floor(now / MINUTE) * MINUTE;
  return {
    store: id,
    root: `/work/${id}.hs2`,
    name,
    checkouts: [checkout],
    epoch_ms: 1,
    revision,
    state: word,
    activity: {
      word,
      active_since_ms: now - 30 * MINUTE,
      stopped_at_ms: null,
      last_progress_ms: now - (word === 'stalled' ? 12 : 2) * MINUTE,
      idle_deadline_ms: null,
      stalled: word === 'stalled' ? [{ slug: 'STW-4K2P', worker: 'worker-2' }] : [],
      halted: [],
      causes: [],
    },
    claims: [],
    workers: { live: word === 'active' ? 2 : 0, expected: 1 },
    waiting: { count: 0, tickets: [] },
    rates: {
      tokens: {
        window_minutes: 5,
        minutes: [{ start_ms: minute, tokens: 8_000 }],
        peak_per_minute: 6_200,
      },
      tickets_updated: { window_minutes: 60, at_ms: [now - 5 * MINUTE, now - 20 * MINUTE], peak_per_hour: 20 },
    },
  };
}

interface Host {
  now: number;
  settings: { enabled: boolean | null; idle_minutes: number | null };
  reads: number;
  puts: unknown[];
  pollDown: boolean;
  sockets: WebSocketRoute[];
  refuseSockets: boolean;
}

const settingsView = (configured: Host['settings']) => ({
  defaults: { enabled: true, idle_minutes: 15 },
  configured,
  effective: { enabled: configured.enabled ?? true, idle_minutes: configured.idle_minutes ?? 15 },
});

async function openStandbyProject(page: Page, settings: Host['settings'] = { enabled: false, idle_minutes: null }) {
  const host: Host = {
    now: Date.now(),
    settings,
    reads: 0,
    puts: [],
    pollDown: false,
    sockets: [],
    refuseSockets: false,
  };
  await page.routeWebSocket(/\/ws\/sync(?:\?|$)/, (socket) => {
    if (host.refuseSockets) {
      void socket.close();
      return;
    }
    host.sockets.push(socket);
  });
  await openDemoProject(page, false, 1, undefined, undefined, {
    liveState: () => ({
      epoch_ms: 1,
      stores: [
        // Two stores linked to the open demo tab, and one the server still hosts for a project whose
        // tab is closed: standby must never show the closed one (HS2-JGW031).
        store('hs2', 'Hot Sheet 2', 'demo-checkout', 'active', host.now),
        store('stw', 'Small Tale Website', 'demo-checkout', 'active', host.now),
        store('kerf', 'Kerf', 'closed-checkout', 'active', host.now),
      ],
    }),
    standbySettings: {
      get: () => {
        host.reads += 1;
        return settingsView(host.settings);
      },
      put: (body) => {
        host.puts.push(body);
        host.settings = body as Host['settings'];
        return settingsView(host.settings);
      },
    },
    pollDown: () => host.pollDown,
  });
  await expect(page.getByRole('tab', { name: /demo/ })).toBeVisible();
  return host;
}

const standby = (page: Page) => page.locator('[data-component="standby-screen"]');
const capture = (page: Page, name: string) =>
  page.screenshot({ path: `target/visual-captures/hs2-eqvztz-${name}.png` });

/** Every region of the display lies inside the viewport, and nothing scrolls sideways. */
async function expectFits(page: Page) {
  const size = page.viewportSize()!;
  const boxes = await standby(page)
    .locator(
      '.standby-screen__top, .standby-screen__gauge, .standby-screen__count, [data-role="standby-alert"], [data-role="standby-row"], [data-role="standby-freshness"]',
    )
    .evaluateAll((nodes) =>
      nodes.map((node) => {
        const rect = node.getBoundingClientRect();
        return { role: node.className, left: rect.left, top: rect.top, right: rect.right, bottom: rect.bottom };
      }),
    );
  // Drift moves the content at most 16px.
  for (const box of boxes) {
    expect(box.left, box.role).toBeGreaterThanOrEqual(-16);
    expect(box.top, box.role).toBeGreaterThanOrEqual(-16);
    expect(box.right, box.role).toBeLessThanOrEqual(size.width + 16);
    expect(box.bottom, box.role).toBeLessThanOrEqual(size.height + 16);
  }
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
}

test.describe('standby (HS2-EQVZTZ)', () => {
  test('the moon forces standby over every open project, even from a project tab, and a click or a key exits (HS2-JGW031)', async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await openStandbyProject(page);
    const moon = page.getByRole('button', { name: 'Enter Standby' });
    await expect(moon).toBeVisible();
    // The moon sits in the left rail, directly before its collapse toggle (approved in m1).
    const rail = page.getByRole('complementary', { name: 'Sidebar rail' });
    await expect(rail.getByRole('button', { name: 'Enter Standby' })).toBeVisible();
    const toggle = await rail.getByRole('button', { name: 'Hide Project Sidebar' }).boundingBox();
    const moonBox = await moon.boundingBox();
    expect(moonBox && toggle).toBeTruthy();
    expect(Math.abs(moonBox!.y + moonBox!.height / 2 - (toggle!.y + toggle!.height / 2))).toBeLessThan(2);
    expect(toggle!.x - (moonBox!.x + moonBox!.width)).toBeGreaterThanOrEqual(0);
    expect(toggle!.x - (moonBox!.x + moonBox!.width)).toBeLessThan(16);
    for (const scheme of ['light', 'dark'] as const) {
      await page.emulateMedia({ colorScheme: scheme });
      await page.screenshot({
        path: `target/visual-captures/hs2-eqvztz-moon-${scheme}.png`,
        clip: { x: 0, y: 0, width: 400, height: 160 },
      });
    }
    await page.emulateMedia({ colorScheme: 'light' });
    // While the rail is collapsed the moon moves with the toggle into the workspace toolbar.
    await rail.getByRole('button', { name: 'Hide Project Sidebar' }).click();
    await expect(page.getByRole('button', { name: 'Show Project Sidebar' })).toBeVisible();
    await expect(moon).toBeVisible();
    await page.screenshot({
      path: 'target/visual-captures/hs2-eqvztz-moon-collapsed.png',
      clip: { x: 0, y: 0, width: 400, height: 160 },
    });
    await page.getByRole('button', { name: 'Show Project Sidebar' }).click();

    await moon.click();
    const screen = standby(page);
    await expect(screen).toBeVisible();
    await expect(screen).toHaveAttribute('aria-label', 'Standby');
    // Entered from a project tab, it still shows every open project, and nothing from closed ones.
    await expect(screen.locator('[data-role="standby-scope"]')).toHaveText('All projects');
    await expect(screen.getByRole('img', { name: /^Token rate: 3\.2k tokens\/min/ })).toBeVisible();
    await expect(screen.getByRole('img', { name: 'Tickets updated: 4' })).toBeVisible();
    await expect(screen.locator('[data-role="standby-row"]')).toHaveCount(2);
    await expect(screen.locator('[data-role="standby-row"]').first()).toContainText('Hot Sheet 2');
    await expect(screen).not.toContainText('Kerf');
    await expect(screen.locator('[data-role="standby-alert"]')).toHaveCount(0);
    // Solid black over the whole viewport, above the app.
    const box = await screen.boundingBox();
    expect(box).toMatchObject({ x: 0, y: 0, width: 1440, height: 900 });
    expect(await screen.evaluate((node) => getComputedStyle(node).backgroundColor)).toBe('rgb(0, 0, 0)');
    expect(await screen.evaluate((node) => node.matches(':popover-open'))).toBe(true);
    await expectFits(page);
    await capture(page, 'open-projects-wide');
    // Wide to medium, narrow, phone portrait, and back: the layout follows the screen.
    for (const [width, height, name] of [
      [1024, 768, 'open-projects-medium'],
      [700, 900, 'open-projects-narrow'],
      [390, 844, 'open-projects-phone'],
      [1440, 900, 'open-projects-wide-again'],
    ] as const) {
      await page.setViewportSize({ width, height });
      await expect(screen).toHaveJSProperty('offsetWidth', width);
      await expectFits(page);
      await capture(page, name);
    }

    await screen.click({ position: { x: 700, y: 450 } });
    await expect(screen).toHaveCount(0);

    // A key exits and never reaches the app beneath (the shortcut for a new ticket does nothing).
    await moon.click();
    await expect(standby(page)).toBeVisible();
    await page.keyboard.press('n');
    await expect(standby(page)).toHaveCount(0);
    await expect(page.getByRole('dialog')).toHaveCount(0);
  });

  test('enters after the idle threshold, shows all projects from the grid, and lights a stalled project', async ({
    page,
  }) => {
    await page.clock.install();
    await page.setViewportSize({ width: 1440, height: 900 });
    const host = await openStandbyProject(page, { enabled: true, idle_minutes: 1 });
    await page.getByRole('button', { name: 'Workspace Grid' }).click();
    await page.mouse.move(10, 10);
    await page.clock.fastForward(50_000);
    await expect(standby(page)).toHaveCount(0);
    await page.clock.fastForward(15_000);
    const screen = standby(page);
    await expect(screen).toBeVisible();
    await expect(screen.locator('[data-role="standby-scope"]')).toHaveText('All projects');
    await expect(screen.locator('[data-role="standby-row"]')).toHaveCount(2);
    await expect(screen.getByRole('img', { name: 'Tickets updated: 4' })).toBeVisible();
    await expect(screen.locator('[data-role="standby-freshness"]')).toHaveAttribute('data-live', 'true');
    await expectFits(page);
    await capture(page, 'all-clear-wide');

    // A pushed live-state change brightens only the problem: one amber callout and its row.
    await expect.poll(() => host.sockets.length).toBeGreaterThan(0);
    const stalled = store('stw', 'Small Tale Website', 'demo-checkout', 'stalled', host.now, 2);
    for (const socket of host.sockets)
      socket.send(JSON.stringify({ cursor: 1, store: 'stw', kind: 'live_state', id: 'stw', slug: '', live: stalled }));
    const alert = screen.locator('[data-role="standby-alert"]');
    await expect(alert).toHaveCount(1);
    await expect(alert).toContainText('Small Tale Website stalled');
    await expect(screen.locator('[data-role="standby-row"][data-alert="true"]')).toHaveCount(1);
    await expect(screen.locator('[data-role="standby-row"][data-alert="true"]')).toContainText('Small Tale Website');
    const amber = await alert.evaluate((node) => getComputedStyle(node).color);
    expect(amber).toBe('rgb(255, 167, 51)');
    await expectFits(page);
    await capture(page, 'attention-wide');
    await page.setViewportSize({ width: 390, height: 844 });
    await page.clock.runFor(100);
    await expect(screen).toHaveJSProperty('offsetWidth', 390);
    await expectFits(page);
    await capture(page, 'attention-phone');
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.clock.runFor(100);
    await expect(screen).toHaveJSProperty('offsetWidth', 1440);

    // The stream drops and stays down: past the reconnect grace the freshness line says so.
    host.pollDown = true;
    host.refuseSockets = true;
    for (const socket of host.sockets) await socket.close();
    await page.clock.fastForward(45_000);
    const freshness = screen.locator('[data-role="standby-freshness"]');
    await expect(freshness).toHaveAttribute('data-live', 'false');
    await expect(freshness).toContainText('connection lost');
    await capture(page, 'connection-lost-wide');
  });

  test('App Settings saves the machine-wide standby settings', async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 800 });
    const host = await openStandbyProject(page, { enabled: null, idle_minutes: null });
    await page.getByLabel('Settings View').click();
    await page.getByRole('button', { name: 'General', exact: true }).click();
    const section = page.locator('[data-component="standby-settings"]');
    await expect(section.getByRole('heading', { name: 'Standby' })).toBeVisible();
    const enabled = section.getByRole('checkbox', { name: 'Enter standby when idle' });
    await expect(enabled).toBeChecked();
    const idle = section.locator('kui-select[name="standby-idle-minutes"]');
    await expect(idle).toHaveJSProperty('value', '15');
    await idle.locator('.kui-select__trigger').click();
    await idle.locator('.kui-select__option[data-value="30"]').click();
    await expect.poll(() => host.puts.at(-1)).toEqual({ enabled: true, idle_minutes: 30 });
    await enabled.uncheck();
    await expect.poll(() => host.puts.at(-1)).toEqual({ enabled: false, idle_minutes: 30 });
    await expect(idle).toHaveAttribute('disabled', '');
  });

  test('a settings change made elsewhere reaches the open client by a pushed event, without polling (HS2-A5BTTK)', async ({
    page,
  }) => {
    await page.clock.install();
    await page.setViewportSize({ width: 1280, height: 800 });
    const host = await openStandbyProject(page, { enabled: false, idle_minutes: null });
    await expect.poll(() => host.sockets.length).toBeGreaterThan(0);
    // One read when the project opens, one for its change stream's initial resync.
    await expect.poll(() => host.reads).toBe(2);
    // Standby is off: a long idle stretch neither enters standby nor re-reads the settings.
    await page.mouse.move(10, 10);
    const settled = host.reads;
    await page.clock.fastForward(30 * 60_000);
    await expect(standby(page)).toHaveCount(0);
    expect(host.reads).toBe(settled);

    // The CLI (or another device) turns standby on with a one-minute threshold; the server pushes
    // the cursored event, and the open client re-reads once and re-arms its idle timer.
    host.settings = { enabled: true, idle_minutes: 1 };
    for (const socket of host.sockets)
      socket.send(JSON.stringify({ cursor: 1, store: '', kind: 'standby_settings', id: '', slug: '' }));
    await expect.poll(() => host.reads).toBe(settled + 1);
    await page.mouse.move(20, 20);
    await page.clock.fastForward(65_000);
    await expect(standby(page)).toBeVisible();
    expect(host.reads).toBe(settled + 1);
  });

  test('leaving standby shows what happened while you were away, and an uneventful standby exits silently (HS2-JJ9RYN)', async ({
    page,
  }) => {
    await page.clock.install();
    await page.setViewportSize({ width: 1440, height: 900 });
    const host = await openStandbyProject(page);
    let completedAt = 0;
    // The server's `GET /checkouts/{id}/confidence-report` shape; one completion during standby.
    await page.route('**/checkouts/demo-checkout/confidence-report', (route) =>
      route.fulfill({
        json: {
          completions: 1,
          scored: 0,
          bands: [],
          events: completedAt
            ? [{ slug: 'HS2-M1', completed_at: new Date(completedAt).toISOString(), outcome: 'pending' }]
            : [],
        },
      }),
    );
    await expect.poll(() => host.sockets.length).toBeGreaterThan(0);
    const moon = page.getByRole('button', { name: 'Enter Standby' });
    const digest = page.locator('[data-away-digest]');

    // Nothing happens: the display exits silently.
    await moon.click();
    await expect(standby(page)).toBeVisible();
    await page.clock.fastForward(10 * MINUTE);
    await standby(page).click({ position: { x: 700, y: 450 } });
    await expect(standby(page)).toHaveCount(0);
    await page.clock.runFor(1_000);
    await expect(digest).toHaveCount(0);

    // Overnight: a store starts waiting on the user, and a ticket completes.
    await moon.click();
    await expect(standby(page)).toBeVisible();
    await page.clock.fastForward(30 * MINUTE);
    const waitingAt = await page.evaluate(() => Date.now());
    completedAt = waitingAt + MINUTE;
    const waiting = {
      ...store('stw', 'Small Tale Website', 'demo-checkout', 'active', host.now, 2),
      state: 'waiting_on_you',
      waiting: {
        count: 1,
        tickets: [{ ticket: 'HS2-M1', slug: 'HS2-M1', title: 'Synthetic question', reason: 'feedback' }],
      },
    };
    for (const socket of host.sockets)
      socket.send(JSON.stringify({ cursor: 1, store: 'stw', kind: 'live_state', id: 'stw', slug: '', live: waiting }));
    await page.clock.fastForward(3 * 60 * MINUTE);
    await standby(page).click({ position: { x: 700, y: 450 } });
    await expect(digest).toBeVisible();
    await expect(digest.getByText('While you were away')).toBeVisible();
    await expect(digest.locator('[data-away-tile="completed"]')).toContainText('1');
    await expect(digest.locator('[data-away-tile="need-you"]')).toHaveAttribute('data-attention', 'true');
    const row = digest.locator('[data-away-event="waiting"]');
    await expect(row).toContainText('Small Tale Website waited on you from');
    await expect(row).toContainText('HS2-M1');
    await expect(digest.getByRole('button', { name: 'Show What Needs Me' })).toBeVisible();
    for (const [width, height, name] of [
      [1440, 900, 'wide'],
      [1024, 768, 'medium'],
      [390, 844, 'narrow'],
      [1440, 900, 'wide-again'],
    ] as const) {
      await page.setViewportSize({ width, height });
      // Let the dialog's enter motion finish before measuring and capturing.
      await page.clock.runFor(1_000);
      await digest.evaluate((node) => Promise.all(node.getAnimations({ subtree: true }).map((item) => item.finished)));
      const box = await digest.boundingBox();
      expect(box!.x).toBeGreaterThanOrEqual(0);
      expect(box!.x + box!.width).toBeLessThanOrEqual(width);
      await page.screenshot({ path: `target/visual-captures/hs2-jj9ryn-away-digest-${name}.png` });
    }

    // Answer opens the waiting ticket in its project and closes the digest.
    const search = page.waitForRequest(
      (request) => request.url().includes('/checkouts/demo-checkout/tickets') && request.url().includes('HS2-M1'),
    );
    await row.getByRole('button', { name: 'Answer' }).click();
    await search;
    await expect(digest).toHaveCount(0);

    // Another eventful standby: "Show What Needs Me" opens cross-project Stats.
    await moon.click();
    await expect(standby(page)).toBeVisible();
    const stalled = store('stw', 'Small Tale Website', 'demo-checkout', 'stalled', host.now, 3);
    for (const socket of host.sockets)
      socket.send(
        JSON.stringify({
          cursor: 2,
          store: 'stw',
          kind: 'live_state',
          id: 'stw',
          slug: '',
          live: { ...stalled, waiting: waiting.waiting },
        }),
      );
    await page.clock.fastForward(5 * MINUTE);
    await page.keyboard.press('Escape');
    await expect(digest).toBeVisible();
    await expect(digest.locator('[data-away-event="stalled"]')).toContainText('Small Tale Website stalled at');
    await digest.getByRole('button', { name: 'Show What Needs Me' }).click();
    await expect(digest).toHaveCount(0);
    await expect(page.locator('[data-component="stats-right-now"]')).toBeVisible();

    // Dismiss and the close button both close it.
    await moon.click();
    const recovered = store('stw', 'Small Tale Website', 'demo-checkout', 'active', host.now, 4);
    for (const socket of host.sockets)
      socket.send(
        JSON.stringify({
          cursor: 3,
          store: 'stw',
          kind: 'live_state',
          id: 'stw',
          slug: '',
          live: { ...recovered, activity: { ...recovered.activity, stalled: [{ slug: 'STW-9', worker: 'w' }] } },
        }),
      );
    await page.clock.fastForward(5 * MINUTE);
    await page.keyboard.press('Escape');
    await expect(digest).toBeVisible();
    await digest.getByRole('button', { name: 'Dismiss' }).click();
    await expect(digest).toHaveCount(0);
  });
});
