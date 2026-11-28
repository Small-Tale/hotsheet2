import { spawnSync } from 'node:child_process';

import { expect, type Page, test } from '@playwright/test';

import { withoutGitRepositoryEnv } from '../scripts/repository-env.mjs';
import { remoteProjectId } from '../src/remote-projects/identity';
import { demoHostHealth, demoLiveState } from '../src/ux-demo/live-section-fixtures';
import { openMockedProject, projectTab } from './open-mocked-project';
import { showProjectSidebarAt } from './project-sidebar-layout';
import { proxyFetch, realTicketServer } from './real-ticket-server';

type Server = Awaited<ReturnType<typeof realTicketServer>>;

/** Open the real app on an isolated server through the dev proxy, as archive-load-more does. */
async function openProject(
  page: Page,
  server: Server,
  beforeLoad?: () => Promise<unknown>,
  /** The browser's id for the project: a `remote:` id opens it as a cross-server tab (HS2-4R99XY). */
  projectId = server.checkoutId,
) {
  const apiPath = `/__hotsheet/project-api/${encodeURIComponent(projectId)}`;
  await page.route(/\/__hotsheet\/(?:remote\/)?projects\/open$/, (route) =>
    route.fulfill({
      status: 201,
      json: { id: projectId, root: server.root, name: 'live', stores: [server.store], apiPath },
    }),
  );
  await page.route(`**${apiPath}/**`, async (route) => {
    const incoming = new URL(route.request().url()),
      path = incoming.pathname.slice(apiPath.length);
    try {
      const response = await proxyFetch(route, {
        url: `${server.url}${path}${incoming.search}`,
        headers: { ...route.request().headers(), 'X-Hotsheet-Secret': server.secret },
        timeout: 60_000,
      });
      await route.fulfill({ response });
    } catch {
      await route.abort().catch(() => undefined);
    }
  });
  // Routes registered here win over the project proxy above (Playwright tries the newest first).
  await beforeLoad?.();
  await page.goto('/');
  await openMockedProject(page);
}

test('HS2-Z9DAYD keeps a full battery quiet and shows a below-50% warning in Live', async ({ page }) => {
  test.setTimeout(120_000);
  const server = await realTicketServer();
  try {
    await page.routeWebSocket(/.*/, (route) => {
      void route.close();
    });
    const highCharge = demoHostHealth('normal');
    highCharge.signals.power.on_battery = true;
    let lowCharge = false;
    await page.setViewportSize({ width: 1280, height: 900 });
    await openProject(page, server, () =>
      page.route('**/host-health', (route) =>
        route.fulfill({ json: lowCharge ? demoHostHealth('battery') : highCharge }),
      ),
    );
    const sidebar = page.locator('section[aria-label="Project sidebar"]').first();
    const live = sidebar.locator('[data-component="project-live-section"]');
    await expect(live).toBeVisible({ timeout: 30_000 });
    await expect(live.locator('[data-host-callout="power"]')).toHaveCount(0);
    await sidebar.screenshot({ path: 'target/visual-captures/hs2-z9dayd-battery-100-sidebar-1280.png' });
    lowCharge = true;
    await page.reload();
    await expect(live.locator('[data-host-callout="power"]')).toContainText('On battery · 49%', {
      timeout: 30_000,
    });
    for (const width of [1280, 760, 390, 1280]) {
      await showProjectSidebarAt(page, width);
      const callout = live.locator('[data-host-callout="power"]');
      await expect(callout).toBeVisible();
      await callout.scrollIntoViewIfNeeded();
      await live.screenshot({ path: `target/visual-captures/hs2-z9dayd-battery-49-${width}.png` });
      if (width === 390) await page.screenshot({ path: 'target/visual-captures/hs2-z9dayd-battery-49-page-390.png' });
    }
    await page.emulateMedia({ colorScheme: 'dark' });
    await live.screenshot({ path: 'target/visual-captures/hs2-z9dayd-battery-49-dark-1280.png' });
  } finally {
    await server.stop(page);
  }
});

// HS2-F788GW: the project sidebar's Live section, from the server's real `/live-state` and the change
// stream, compact by default; the header action switches to the detail rows and the choice survives a
// reload, and the section sits under the completion chart.
test('project sidebar Live section is compact by default and remembers the detail mode (HS2-F788GW)', async ({
  page,
}) => {
  test.setTimeout(120_000);
  const server = await realTicketServer();
  try {
    const ready = await server.request<{ id: string }>('/tickets', 'POST', { title: 'Ready work', category: 'task' });
    await server.request(`/tickets/${ready.id}`, 'PATCH', { up_next: true });
    // Change delivery falls back from the (unproxied) WebSocket to the forwarded long poll.
    await page.routeWebSocket(/.*/, (route) => {
      void route.close();
    });
    await page.setViewportSize({ width: 1280, height: 900 });
    await openProject(page, server);

    const sidebar = page.locator('section[aria-label="Project sidebar"]').first();
    const live = sidebar.locator('[data-component="project-live-section"]');
    await expect(live).toHaveAttribute('data-mode', 'compact', { timeout: 30_000 });
    // Under the completion chart, above the repository summary.
    const order = await sidebar.evaluate((root) =>
      ['project-summary', 'project-live-section', 'repository-summary'].map((name) => {
        const node = root.querySelector(`[data-component="${name}"]`);
        return node ? node.getBoundingClientRect().top : -1;
      }),
    );
    expect(order[0]).toBeGreaterThanOrEqual(0);
    expect(order[1]).toBeGreaterThan(order[0]);
    expect(order[2]).toBeGreaterThan(order[1]);

    // Compact: two bare chip gauges named by their titles. A quiet project shows no chips; queue
    // runway is gone even with Up Next work (HS2-5A83MR).
    const gauges = live.locator('svg.glance-gauge');
    await expect(gauges).toHaveCount(2);
    await expect(gauges.nth(0)).toHaveAttribute('data-variant', 'bare');
    await expect(gauges.nth(0)).toHaveAccessibleName(/^Token rate: 0 tokens\/min/);
    await expect(gauges.nth(1)).toHaveAccessibleName(/^Tickets updated: \d+ tickets\/h/);
    await expect(
      live.locator('[data-live-chip="runway"], [data-live-chip="waiting"], [data-live-chip="overrun"]'),
    ).toHaveCount(0);
    await expect(live.locator('[data-live-row]')).toHaveCount(0);

    // No header bar (HS2-9J6PPR): clicking the gauges themselves switches to the detail rows, and the
    // one toggle names the mode it switches to; the gauges keep their hover titles.
    await expect(live.locator('[data-component="list-header"]')).toHaveCount(0);
    await expect(live.getByText('Live', { exact: true })).toHaveCount(0);
    const toggle = live.getByRole('button', { name: /^Show live details\. / });
    await expect(toggle).toHaveAttribute('aria-expanded', 'false');
    await expect(gauges.nth(0).locator('title')).toHaveText(/^Token rate: 0 tokens\/min/);
    await gauges.nth(1).click();
    await expect(live).toHaveAttribute('data-mode', 'expanded');
    await expect(live.getByRole('button', { name: /^Show compact live summary\. / })).toHaveAttribute(
      'aria-expanded',
      'true',
    );
    await expect(live.locator('svg.glance-gauge[data-variant="full"]')).toHaveCount(2);
    await expect(live.locator('[data-live-row="state"]')).toBeVisible();
    await expect(live.locator('[data-live-row="runway"], [data-live-row="waiting"]')).toHaveCount(0);
    // Expanded gauges sit centred in the section (HS2-80SWPT): the pair's midpoint is the row's.
    const centring = await live.locator('[data-live-gauges]').evaluate((row) => {
      const box = row.getBoundingClientRect(),
        dials = [...row.querySelectorAll('svg.glance-gauge')].map((dial) => dial.getBoundingClientRect());
      const left = Math.min(...dials.map((dial) => dial.left)),
        right = Math.max(...dials.map((dial) => dial.right));
      return { row: box.left + box.width / 2, pair: (left + right) / 2, width: right - left };
    });
    expect(centring.width).toBeGreaterThan(100);
    expect(Math.abs(centring.pair - centring.row)).toBeLessThanOrEqual(1);
    // Captures show the resting state: no hover fill or focus ring.
    await page.mouse.move(640, 880);
    await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
    await expect(live.locator('button')).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
    await page.screenshot({ path: 'target/visual-captures/hs2-f788gw-live-expanded-1280.png' });

    // The choice persists per viewer across a reload, which restores the open project.
    await page.reload();
    await expect(live).toHaveAttribute('data-mode', 'expanded', { timeout: 30_000 });
    // The keyboard path: focus the content's toggle and press Enter, then Space toggles back.
    await live.getByRole('button', { name: /^Show compact live summary\. / }).focus();
    await page.keyboard.press('Enter');
    await expect(live).toHaveAttribute('data-mode', 'compact');
    await live.getByRole('button', { name: /^Show live details\. / }).focus();
    await page.keyboard.press('Space');
    await expect(live).toHaveAttribute('data-mode', 'expanded');
    // Clicking the detail rows (not only the gauges) toggles too.
    await live.locator('[data-live-row="state"]').click();
    await expect(live).toHaveAttribute('data-mode', 'compact');
    await expect(live.locator('[data-live-row]')).toHaveCount(0);
    await page.mouse.move(640, 880);
    await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
    await expect(live.locator('button')).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
    await page.screenshot({ path: 'target/visual-captures/hs2-f788gw-live-compact-1280.png' });
  } finally {
    await server.stop(page);
  }
});

// HS2-57BYEA: `main` CI reported to the real server (POST ci-status, HS2-M8HHEA) reaches the Live section
// through the change stream: a failing run is a danger chip in compact mode, a passing one is no chip but
// an expanded row, wide to medium to wide.
test('project sidebar Live section shows reported main CI as a chip only when it fails and as a row (HS2-57BYEA)', async ({
  page,
}) => {
  test.setTimeout(120_000);
  const server = await realTicketServer();
  try {
    await page.routeWebSocket(/.*/, (route) => {
      void route.close();
    });
    await page.setViewportSize({ width: 1280, height: 900 });
    await openProject(page, server);
    const sidebar = page.locator('section[aria-label="Project sidebar"]').first();
    const live = sidebar.locator('[data-component="project-live-section"]');
    await expect(live).toHaveAttribute('data-mode', 'compact', { timeout: 30_000 });
    await expect(live.locator('[data-live-chip="ci"]')).toHaveCount(0);

    const now = Date.now();
    const report = (status: 'failure' | 'success', sha: string, startedMinutesAgo: number) =>
      server.request(`/checkouts/${server.checkoutId}/ci-status`, 'POST', {
        status,
        branch: 'main',
        sha,
        workflow: 'CI',
        started_at_ms: now - startedMinutesAgo * 60_000,
        finished_at_ms: now - (startedMinutesAgo - 6) * 60_000,
      });
    await report('failure', '9f8e7d6c5b4a', 18);
    const chip = live.locator('[data-live-chip="ci"]');
    await expect(chip).toBeVisible({ timeout: 30_000 });
    await expect(chip).toHaveAttribute('title', 'main CI failed · 9f8e7d6');
    await expect(chip).toHaveAttribute('data-tone', 'danger');
    await expect(chip.locator('[data-lucide="circle-x"]')).toBeVisible();
    await page.mouse.move(640, 880);
    for (const width of [1280, 1024, 1280]) {
      await page.setViewportSize({ width, height: 900 });
      await expect(chip).toBeVisible();
      await live.screenshot({ path: `target/visual-captures/hs2-57byea-ci-chip-compact-${width}.png` });
    }

    // A newer passing run clears the chip; the expanded rows name it.
    await report('success', '1a2b3c4d5e6f', 8);
    await expect(chip).toHaveCount(0, { timeout: 30_000 });
    await live.getByRole('button', { name: /^Show live details\. / }).click();
    await expect(live).toHaveAttribute('data-mode', 'expanded');
    const row = live.locator('[data-live-row="ci"]');
    await expect(row).toContainText('main CI passed · 1a2b3c4');
    await expect(row).toContainText(/\dm ago/u);
    await expect(row.locator('[data-lucide="circle-check"]')).toBeVisible();
    await page.mouse.move(640, 880);
    await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
    for (const width of [1280, 1024, 1280]) {
      await page.setViewportSize({ width, height: 900 });
      await expect(row).toBeVisible();
      const fits = await row.evaluate((node) => node.scrollWidth <= node.clientWidth + 1);
      expect(fits).toBe(true);
      await live.screenshot({ path: `target/visual-captures/hs2-57byea-ci-row-expanded-${width}.png` });
    }
  } finally {
    await server.stop(page);
  }
});

// HS2-505PHC: the compact `main` CI chip names itself. A running run reported to the real server reads
// `CI · m:ss` from its reported start and advances each second from the render-only display clock without a
// single request; queued and failed runs read `CI · queued` and `CI failed`. Captured wide, medium, narrow,
// and wide again, in light and dark.
test('HS2-505PHC labels the Live CI chip with its elapsed time and ticks without requests', async ({
  page,
}, testInfo) => {
  test.setTimeout(150_000);
  const server = await realTicketServer();
  try {
    await page.routeWebSocket(/.*/, (route) => {
      void route.close();
    });
    await page.setViewportSize({ width: 1280, height: 900 });
    const requests: string[] = [];
    page.on('request', (request) => {
      const url = new URL(request.url());
      if (url.pathname.startsWith('/__hotsheet/')) requests.push(`${request.method()} ${url.pathname}`);
    });
    await openProject(page, server);
    const sidebar = page.locator('section[aria-label="Project sidebar"]').first();
    const live = sidebar.locator('[data-component="project-live-section"]');
    await expect(live).toHaveAttribute('data-mode', 'compact', { timeout: 30_000 });
    const chip = live.locator('[data-live-chip="ci"]');
    await expect(chip).toHaveCount(0);

    const report = (status: 'in_progress' | 'queued' | 'failure', startedAtMs: number, finishedAtMs?: number) =>
      server.request(`/checkouts/${server.checkoutId}/ci-status`, 'POST', {
        status,
        branch: 'main',
        sha: '9f8e7d6c5b4a',
        workflow: 'CI',
        started_at_ms: startedAtMs,
        ...(finishedAtMs === undefined ? {} : { finished_at_ms: finishedAtMs }),
      });
    // Queued first: a word, no clock. (A later report only replaces it when it did not start earlier.)
    await report('queued', Date.now() - 90_000);
    await expect(chip).toHaveText('CI · queued', { timeout: 30_000 });
    await expect(chip).toHaveAttribute('title', 'main CI queued · 9f8e7d6');
    await expect(chip.locator('[data-lucide="clock"]')).toBeVisible();

    // Running from a start 65 seconds ago: `CI · 1:05` or a second or two later, then it counts up.
    await report('in_progress', Date.now() - 65_000);
    await expect(chip).toHaveText(/^CI · 1:0\d$/u, { timeout: 30_000 });
    await expect(chip).toHaveAttribute('data-tone', 'neutral');
    await expect(chip).toHaveAttribute('title', /^main CI running for 1:\d\d · 9f8e7d6$/u);
    await expect(chip.locator('[data-lucide="loader-circle"]')).toBeVisible();
    const seconds = async () => {
      const match = /(\d+):(\d\d)$/u.exec((await chip.textContent()) ?? '');
      return match ? Number(match[1]) * 60 + Number(match[2]) : Number.NaN;
    };
    // Let the report's own change delivery settle, then watch three ticks: the clock adds no request, so
    // the only traffic is the change stream's own long poll (the WebSocket is refused here).
    await expect.poll(seconds).toBeGreaterThan(await seconds());
    const quietFrom = requests.length,
      firstTick = await seconds();
    await expect.poll(seconds, { timeout: 10_000 }).toBeGreaterThanOrEqual(firstTick + 3);
    const whileTicking = requests.slice(quietFrom);
    testInfo.annotations.push({ type: 'requests-while-ticking', description: whileTicking.join(', ') });
    expect(
      whileTicking.filter((request) => !/^GET \/__hotsheet\/project-api\/[^/]+\/ws\/poll$/u.test(request)),
    ).toEqual([]);

    await page.mouse.move(640, 880);
    const capture = async (name: string) => {
      await live.screenshot({ path: `target/visual-captures/hs2-505phc-${name}.png` });
    };
    for (const width of [1280, 1024, 390, 1280]) {
      await showProjectSidebarAt(page, width);
      await expect(chip).toBeVisible();
      await expect(chip).toHaveText(/^CI · \d+:\d\d$/u);
      // The label stays on one line inside the badge at every width.
      const box = await chip.boundingBox();
      expect(box?.height ?? 0).toBeLessThan(32);
      await capture(`running-${width}`);
      if (width === 1280) {
        const sidebarBox = await sidebar.boundingBox();
        if (sidebarBox)
          await page.screenshot({
            path: 'target/visual-captures/hs2-505phc-running-sidebar-top.png',
            clip: {
              x: sidebarBox.x,
              y: sidebarBox.y,
              width: sidebarBox.width,
              height: Math.min(420, sidebarBox.height),
            },
          });
      }
    }
    await page.emulateMedia({ colorScheme: 'dark' });
    await capture('running-dark-1280');

    // Failed: a word in the danger tone, and the clock stops.
    const now = Date.now();
    await report('failure', now - 65_000, now - 1_000);
    await expect(chip).toHaveText('CI failed', { timeout: 30_000 });
    await expect(chip).toHaveAttribute('data-tone', 'danger');
    await expect(chip.locator('[data-lucide="circle-x"]')).toBeVisible();
    await capture('failed-dark-1280');
    await page.emulateMedia({ colorScheme: 'light' });
    await capture('failed-1280');
  } finally {
    await server.stop(page);
  }
});

// HS2-D3JJ49 (replacing HS2-CC7TCB's `CI · unknown`): a run reported running by a reporter nothing follows is
// believed for 2 h after its report; then it cannot be verified and the Live section shows nothing for it, in
// both modes, and the display clock stops. The browser clock is moved past the bound instead of waiting; the
// server stamps the report time and its `verify_by_ms` itself.
test('HS2-D3JJ49 shows nothing for a running CI report that can no longer be verified', async ({ page }) => {
  test.setTimeout(150_000);
  const server = await realTicketServer();
  try {
    await page.routeWebSocket(/.*/, (route) => {
      void route.close();
    });
    await page.setViewportSize({ width: 1280, height: 900 });
    await openProject(page, server);
    const sidebar = page.locator('section[aria-label="Project sidebar"]').first();
    const live = sidebar.locator('[data-component="project-live-section"]');
    await expect(live).toHaveAttribute('data-mode', 'compact', { timeout: 30_000 });
    const chip = live.locator('[data-live-chip="ci"]');

    // Reported running, 5 minutes in, by a reporter that never reports again and no watch follows.
    const answer = await server.request<{ follow: string; ci: { verification: string; verify_by_ms: number } }>(
      `/checkouts/${server.checkoutId}/ci-status`,
      'POST',
      {
        status: 'in_progress',
        branch: 'main',
        sha: '8f6c8d81f0a1',
        workflow: 'CI',
        started_at_ms: Date.now() - 5 * 60_000,
      },
    );
    expect(answer.follow).not.toBe('started');
    expect(answer.ci.verification).toBe('verified');
    await expect(chip).toHaveText(/^CI · 5:\d\d$/u, { timeout: 30_000 });

    // Two hours and ten minutes later, on this machine's clock: the next display tick removes the chip.
    await page.clock.setFixedTime(Date.now() + (2 * 60 + 10) * 60_000);
    await expect(chip).toHaveCount(0, { timeout: 30_000 });
    await expect(live).not.toContainText('unknown');
    await live.getByRole('button', { name: /^Show live details\. / }).click();
    await expect(live).toHaveAttribute('data-mode', 'expanded');
    await expect(live.locator('[data-live-row="ci"]')).toHaveCount(0);
  } finally {
    await server.stop(page);
  }
});

// HS2-D3JJ49: several runs in flight (two workflows of one push) each get their own chip and expanded row in the
// shipped sidebar, named by their workflow, and each ticks. Captured wide, medium, narrow and wide again, light
// and dark, in Chromium and WebKit.
test('HS2-D3JJ49 shows each running CI run as its own chip and row', async ({ page, browserName }) => {
  test.setTimeout(150_000);
  const server = await realTicketServer();
  try {
    await page.setViewportSize({ width: 1280, height: 900 });
    await openProject(page, server);
    const sidebar = page.locator('section[aria-label="Project sidebar"]').first();
    const live = sidebar.locator('[data-component="project-live-section"]');
    await expect(live).toHaveAttribute('data-mode', 'compact', { timeout: 30_000 });
    const chips = live.locator('[data-live-chip="ci"]');
    for (const [workflow, startedAgoMs] of [
      ['CI', 65_000],
      ['Platforms', 190_000],
    ] as const)
      await server.request(`/checkouts/${server.checkoutId}/ci-status`, 'POST', {
        status: 'in_progress',
        branch: 'main',
        sha: '9f8e7d6c5b4a',
        workflow,
        started_at_ms: Date.now() - startedAgoMs,
      });
    await expect(chips).toHaveCount(2, { timeout: 30_000 });
    const platforms = chips.filter({ hasText: /^Platforms · \d+:\d\d$/u }),
      ci = chips.filter({ hasText: /^CI · \d+:\d\d$/u });
    await expect(platforms).toHaveCount(1);
    await expect(ci).toHaveCount(1);
    await expect(platforms).toHaveAttribute('title', /^main Platforms running for \d+:\d\d · 9f8e7d6$/u);
    await expect(platforms).toHaveAttribute('data-ci-clear-workflow', 'Platforms');
    // Both clocks tick.
    const before = await platforms.textContent();
    await expect(platforms).not.toHaveText(before ?? '', { timeout: 5_000 });

    await page.mouse.move(640, 880);
    const capture = async (name: string) => {
      await live.screenshot({ path: `target/visual-captures/hs2-d3jj49-${browserName}-${name}.png` });
    };
    for (const width of [1280, 1024, 390, 1280]) {
      await showProjectSidebarAt(page, width);
      await expect(chips).toHaveCount(2);
      for (const each of await chips.all()) {
        const box = await each.boundingBox();
        expect(box?.height ?? 0).toBeLessThan(32);
      }
      await capture(`chips-${width}`);
    }
    await page.emulateMedia({ colorScheme: 'dark' });
    await capture('chips-dark-1280');

    await live.getByRole('button', { name: /^Show live details\. / }).click();
    await expect(live).toHaveAttribute('data-mode', 'expanded');
    const rows = live.locator('[data-live-row="ci"]');
    await expect(rows).toHaveCount(2);
    await expect(rows.filter({ hasText: /^Platforms running/u })).toHaveCount(1);
    await expect(rows.filter({ hasText: /^CI running/u })).toHaveCount(1);
    await page.mouse.move(640, 880);
    await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
    await capture('rows-dark-1280');
    await page.emulateMedia({ colorScheme: 'light' });
    for (const width of [1280, 390]) {
      await showProjectSidebarAt(page, width);
      await expect(rows).toHaveCount(2);
      // Neither row nor its truncating label overflows: the whole label shows.
      for (const row of await rows.all()) {
        const fits = await row.evaluate((node) =>
          [node, ...node.querySelectorAll('span')].every((part) => part.scrollWidth <= part.clientWidth + 1),
        );
        expect(fits).toBe(true);
      }
      await capture(`rows-${width}`);
    }
  } finally {
    await server.stop(page);
  }
});

// HS2-7PSBHQ, HS2-D3JJ49: Clear CI Status applies per run. Right-clicking one run's chip offers that run; the real
// server forgets it and the pushed live state removes only its chip. Right-clicking elsewhere on the section, or
// the context-menu key on its focused toggle, lists each shown run by name. Captured wide, medium, narrow and wide
// again, light and dark, with the menu open.
test('HS2-7PSBHQ clears one CI run from the Live section context menu', async ({ page, browserName }) => {
  test.setTimeout(150_000);
  const server = await realTicketServer();
  try {
    await page.setViewportSize({ width: 1280, height: 900 });
    await openProject(page, server);
    const sidebar = page.locator('section[aria-label="Project sidebar"]').first();
    const live = sidebar.locator('[data-component="project-live-section"]');
    await expect(live).toHaveAttribute('data-mode', 'compact', { timeout: 30_000 });
    const chips = live.locator('[data-live-chip="ci"]'),
      menu = page.locator('[data-context-menu="live-ci"]'),
      items = menu.getByRole('menuitem');
    const report = (workflow: string) =>
      server.request(`/checkouts/${server.checkoutId}/ci-status`, 'POST', {
        status: 'in_progress',
        branch: 'main',
        sha: '8f6c8d81f0a1',
        workflow,
        started_at_ms: Date.now() - 5 * 60_000,
      });
    await report('CI');
    await report('Platforms');
    await expect(chips).toHaveCount(2, { timeout: 30_000 });
    const platforms = chips.filter({ hasText: /^Platforms · / });

    // Right-click on one chip offers that run alone.
    await platforms.click({ button: 'right' });
    await expect(items).toHaveCount(1);
    await expect(items.first()).toHaveText('Clear CI Status');
    await expect(items.first()).toHaveAttribute('data-ci-clear-workflow', 'Platforms');
    await expect(items.first().locator('[data-lucide="eraser"]')).toBeAttached();
    await page.keyboard.press('Escape');
    await expect(menu).toHaveCount(0);

    // The context-menu key on the focused toggle lists each shown run by name, below the toggle.
    const toggle = live.getByRole('button', { name: /^Show live details\. / });
    await toggle.focus();
    await toggle.dispatchEvent('contextmenu', { clientX: 0, clientY: 0, bubbles: true, cancelable: true });
    // In the section's order: the newest report first.
    await expect(items).toHaveText(['Clear main Platforms Status', 'Clear main CI Status']);
    // Opening the menu does not switch the section's mode.
    await expect(live).toHaveAttribute('data-mode', 'compact');

    const capture = async (name: string) => {
      // The menu fades in; capture it settled.
      if (await menu.count())
        await menu.evaluate((node) =>
          Promise.all(node.getAnimations({ subtree: true }).map((animation) => animation.finished)),
        );
      await page.screenshot({ path: `target/visual-captures/hs2-d3jj49-${browserName}-${name}.png` });
    };
    for (const width of [1280, 1024, 390, 1280]) {
      await page.keyboard.press('Escape');
      await expect(menu).toHaveCount(0);
      await showProjectSidebarAt(page, width);
      // Right-click on the gauges, not a chip: every shown run.
      await live.locator('[data-component="glance-gauge"]').first().click({ button: 'right' });
      await expect(items).toHaveCount(2);
      for (const each of await items.all()) {
        const box = await each.boundingBox();
        expect((box?.x ?? -1) >= 0 && (box?.x ?? 0) + (box?.width ?? 0) <= width).toBe(true);
      }
      await capture(`menu-${width}`);
    }
    await page.emulateMedia({ colorScheme: 'dark' });
    await capture('menu-dark-1280');
    await page.emulateMedia({ colorScheme: 'light' });

    // Clear only Platforms: its chip goes, the CI chip stays, and the server keeps the CI run.
    await items.filter({ hasText: 'Platforms' }).click();
    await expect(menu).toHaveCount(0);
    await expect(chips).toHaveCount(1, { timeout: 30_000 });
    await expect(chips.first()).toHaveText(/^CI · \d+:\d\d$/u);
    const state = await server.request<{ stores: { ci_runs: { workflow: string }[] }[] }>('/live-state');
    expect(state.stores.map((store) => store.ci_runs.map((run) => run.workflow))).toEqual([['CI']]);
    // With one run left, the section's menu offers it unnamed; clearing it leaves no chip or row.
    await chips.first().click({ button: 'right' });
    await expect(items).toHaveText(['Clear CI Status']);
    await items.first().click();
    await expect(chips).toHaveCount(0, { timeout: 30_000 });
    await toggle.click();
    await expect(live).toHaveAttribute('data-mode', 'expanded');
    await expect(live.locator('[data-live-row="ci"]')).toHaveCount(0);
    // Nothing shown: right-click keeps the browser's own menu.
    await live.click({ button: 'right' });
    await expect(menu).toHaveCount(0);
    await page.mouse.move(640, 880);
    await capture('cleared-expanded-1280');
  } finally {
    await server.stop(page);
  }
});

// HS2-4R99XY: a cross-server (remote) project tab shows the same Live gauges and badges as a local one. The
// browser's `remote:<binding>:<checkout>` id never equals the remote server's checkout id, so the section reads
// the tab's own live store by the remote checkout id; a reported failing run still reaches its chip.
test('project sidebar Live section shows gauges and badges for a remote project tab (HS2-4R99XY)', async ({ page }) => {
  test.setTimeout(120_000);
  const server = await realTicketServer();
  try {
    await page.routeWebSocket(/.*/, (route) => {
      void route.close();
    });
    await page.setViewportSize({ width: 1280, height: 900 });
    await openProject(page, server, undefined, remoteProjectId('01M7S5JGSGWFWB3A2WHJ0P2NTC', server.checkoutId));
    const sidebar = page.locator('section[aria-label="Project sidebar"]').first();
    const live = sidebar.locator('[data-component="project-live-section"]');
    await expect(live).toHaveAttribute('data-mode', 'compact', { timeout: 30_000 });
    const gauges = live.locator('svg.glance-gauge');
    await expect(gauges).toHaveCount(2);
    await expect(gauges.nth(0)).toHaveAccessibleName(/^Token rate: 0 tokens\/min/);
    await expect(gauges.nth(1)).toHaveAccessibleName(/^Tickets updated: \d+ tickets\/h/);

    await server.request(`/checkouts/${server.checkoutId}/ci-status`, 'POST', {
      status: 'failure',
      branch: 'main',
      sha: '9f8e7d6c5b4a',
      workflow: 'CI',
      started_at_ms: Date.now() - 18 * 60_000,
      finished_at_ms: Date.now() - 12 * 60_000,
    });
    const chip = live.locator('[data-live-chip="ci"]');
    await expect(chip).toBeVisible({ timeout: 30_000 });
    await expect(chip).toHaveAttribute('data-tone', 'danger');
    for (const width of [1280, 1024, 1280]) {
      await page.setViewportSize({ width, height: 900 });
      await expect(chip).toBeVisible();
      await live.screenshot({ path: `target/visual-captures/hs2-4r99xy-remote-live-${width}.png` });
    }
  } finally {
    await server.stop(page);
  }
});

// HS2-0EQMZZ: a host callout keeps 16px (the 8px grid's next step) below the gauges and badges in both
// modes, wide to medium to wide while the sidebar shows (narrower widths make it a drawer), in the real app with the server's host health replaced
// by a synthetic critical-disk snapshot.
test('keeps 16px between the Live gauges and a host callout in both modes (HS2-0EQMZZ)', async ({ page }) => {
  test.setTimeout(120_000);
  const server = await realTicketServer();
  try {
    await page.routeWebSocket(/.*/, (route) => {
      void route.close();
    });
    await page.setViewportSize({ width: 1280, height: 900 });
    await openProject(page, server, () =>
      page.route('**/host-health', (route) => route.fulfill({ json: demoHostHealth('disk-critical') })),
    );
    const sidebar = page.locator('section[aria-label="Project sidebar"]').first();
    const live = sidebar.locator('[data-component="project-live-section"]');
    const callout = live.locator('[data-host-callout="disk"]');
    await expect(callout).toBeVisible({ timeout: 30_000 });
    const gap = () =>
      live.evaluate((section) => {
        const toggle = section.querySelector('.project-live-section__toggle')!.getBoundingClientRect(),
          banner = section.querySelector('[data-host-callout]')!.getBoundingClientRect();
        return Math.round(banner.top - toggle.bottom);
      });
    for (const width of [1440, 1120, 1440]) {
      await page.setViewportSize({ width, height: 900 });
      for (const mode of ['compact', 'expanded']) {
        if ((await live.getAttribute('data-mode')) !== mode) await live.locator('button').first().click();
        await expect(live).toHaveAttribute('data-mode', mode);
        expect(await gap(), `${mode} at ${width}`).toBe(16);
      }
    }
    await page.mouse.move(640, 880);
    await expect(live.locator('button').first()).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
    await page.screenshot({ path: 'target/visual-captures/hs2-0eqmzz-live-expanded-callout-1440.png' });
    await live.locator('button').first().click();
    await page.mouse.move(640, 880);
    await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
    await expect(live.locator('button').first()).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
    await page.screenshot({ path: 'target/visual-captures/hs2-0eqmzz-live-compact-callout-1440.png' });
  } finally {
    await server.stop(page);
  }
});

// HS2-K4DY5G: a remote project's sidebar callouts describe the remote machine, never this one. The local
// tab's server reports a critical disk and the remote tab's server a battery warning; each tab's sidebar
// shows only its own server's warning, in both directions.
test('a remote project tab shows its own server host warnings, not the local machine (HS2-K4DY5G)', async ({
  page,
}) => {
  test.setTimeout(120_000);
  const server = await realTicketServer();
  try {
    const remoteId = remoteProjectId('01M7S5JGSGWFWB3A2WHJ0P2NTC', server.checkoutId);
    const localPath = `/__hotsheet/project-api/${server.checkoutId}`;
    const remotePath = `/__hotsheet/project-api/${encodeURIComponent(remoteId)}`;
    await page.routeWebSocket(/.*/, (route) => {
      void route.close();
    });
    await page.route(/\/__hotsheet\/folders\/choose$/, (route) =>
      route.fulfill({ json: { path: `${server.root}-remote` } }),
    );
    let opened = 0;
    await page.route(/\/__hotsheet\/(?:remote\/)?projects\/open$/, (route) => {
      opened += 1;
      return route.fulfill({
        status: 201,
        json:
          opened === 1
            ? { id: server.checkoutId, root: server.root, name: 'Launch', stores: [server.store], apiPath: localPath }
            : { id: remoteId, root: remoteId, name: 'Build host · API', stores: [], apiPath: remotePath },
      });
    });
    // Both tabs reach the fixture server, as the bridge reaches a real remote one; only the host health differs.
    for (const [prefix, scenario] of [
      [localPath, 'disk-critical'],
      [remotePath, 'battery'],
    ] as const) {
      await page.route(`**${prefix}/host-health`, (route) => route.fulfill({ json: demoHostHealth(scenario) }));
      await page.route(`**${prefix}/**`, async (route) => {
        const incoming = new URL(route.request().url());
        if (incoming.pathname.endsWith('/host-health')) return route.fallback();
        let path = incoming.pathname.slice(prefix.length);
        if (prefix === remotePath)
          path = path.replace(`/checkouts/${encodeURIComponent(remoteId)}`, `/checkouts/${server.checkoutId}`);
        try {
          const response = await proxyFetch(route, {
            url: `${server.url}${path}${incoming.search}`,
            headers: { ...route.request().headers(), 'X-Hotsheet-Secret': server.secret },
            timeout: 60_000,
          });
          await route.fulfill({ response });
        } catch {
          await route.abort().catch(() => undefined);
        }
      });
    }
    await page.setViewportSize({ width: 1280, height: 900 });
    await page.goto('/');
    await openMockedProject(page);
    await expect(projectTab(page, 'Launch')).toBeVisible();
    await page.getByRole('button', { name: 'Add Project' }).click();
    await page.getByRole('button', { name: 'Local Project' }).click();
    await expect(projectTab(page, 'Build host · API')).toBeVisible({ timeout: 30_000 });

    const live = page
      .locator('section[aria-label="Project sidebar"]')
      .first()
      .locator('[data-component="project-live-section"]');
    // The remote tab is selected: its server's battery warning shows and this machine's disk warning does not.
    await expect(live.locator('[data-host-callout="power"]')).toBeVisible({ timeout: 30_000 });
    await expect(live.locator('[data-host-callout="disk"]')).toHaveCount(0);
    for (const width of [1280, 1120, 1280]) {
      await page.setViewportSize({ width, height: 900 });
      await expect(live.locator('[data-host-callout="power"]')).toBeVisible({ timeout: 10_000 });
      await live.screenshot({ path: `target/visual-captures/hs2-k4dy5g-remote-warning-${width}.png`, timeout: 15_000 });
    }
    // The local tab keeps its own warning and none of the remote server's.
    await projectTab(page, 'Launch').click();
    await expect(live.locator('[data-host-callout="disk"]')).toBeVisible({ timeout: 30_000 });
    await expect(live.locator('[data-host-callout="power"]')).toHaveCount(0);
  } finally {
    await server.stop(page);
  }
});

// HS2-6B9S7P: the Workers view choice names the live worker count and the time since last progress;
// with nobody working it keeps its plain name. The visible badge waits on Kerf KF-170258.
test('Workers view choice names live workers and last progress (HS2-6B9S7P)', async ({ page }) => {
  test.setTimeout(120_000);
  const server = await realTicketServer();
  try {
    await page.routeWebSocket(/.*/, (route) => {
      void route.close();
    });
    await page.setViewportSize({ width: 1280, height: 900 });
    await openProject(page, server);
    const workers = page.locator('[data-segment-value="workers"]').first();
    await expect(workers).toHaveAccessibleName('Workers View', { timeout: 30_000 });
    await expect(workers).toHaveAttribute('title', 'Workers view');

    const ticket = await server.request<{ id: string }>('/tickets', 'POST', {
      title: 'Claimed work',
      category: 'task',
    });
    await server.request(`/tickets/${ticket.id}/claim`, 'POST', { worker: 'worker-a', lease_minutes: 30 });
    await expect(workers).toHaveAccessibleName('Workers View, 1 live', { timeout: 30_000 });
    await expect(workers).toHaveAttribute('title', /^1 worker live · (last progress \S+ ago|no progress yet)$/);
    // No hand-positioned badge joins the choice.
    await expect(workers.locator('.view-mode-switcher__badge')).toHaveCount(0);

    await server.request(`/tickets/${ticket.id}/release`, 'POST', { worker: 'worker-a' });
    await expect(workers).toHaveAccessibleName('Workers View', { timeout: 30_000 });
  } finally {
    await server.stop(page);
  }
});

test('HS2-HWMW0D keeps the real Workers view and edit through worker updates and resize', async ({ page }) => {
  test.setTimeout(180_000);
  let fixtureCli = '';
  let fixtureEnv: NodeJS.ProcessEnv = {};
  const server = await realTicketServer({
    seed: async ({ root, env, cli }) => {
      fixtureCli = cli;
      fixtureEnv = env;
      const initialized = spawnSync('git', ['-C', root, 'init'], {
        env: withoutGitRepositoryEnv(env),
        encoding: 'utf8',
      });
      expect(initialized.status, initialized.stderr).toBe(0);
      const committed = spawnSync(
        'git',
        [
          '-C',
          root,
          '-c',
          'user.name=Hot Sheet fixture',
          '-c',
          'user.email=fixture@example.invalid',
          'commit',
          '--allow-empty',
          '-m',
          'Seed synthetic checkout',
        ],
        { env: withoutGitRepositoryEnv(env), encoding: 'utf8' },
      );
      expect(committed.status, committed.stderr).toBe(0);
    },
  });
  const run = (...args: string[]) => {
    const result = spawnSync(fixtureCli, ['-C', server.store, 'coordination', '--project', server.root, ...args], {
      cwd: server.root,
      env: fixtureEnv,
      encoding: 'utf8',
    });
    expect(result.status, `${args.join(' ')}: ${result.stderr}`).toBe(0);
  };
  try {
    const ticket = await server.request<{ id: string }>('/tickets', 'POST', {
      title: 'Synthetic ticket with a long but readable name for the Workers view',
      category: 'task',
    });
    const workerId = 'synthetic-worker-with-an-intentionally-long-label';
    await server.request(`/tickets/${ticket.id}/claim`, 'POST', { worker: workerId, lease_minutes: 30 });
    run('setup');
    run(
      'register',
      '--role',
      'worker',
      '--id',
      workerId,
      '--session',
      'worker-long-1',
      '--route',
      'human',
      '--lease-seconds',
      '3600',
    );
    for (let number = 1; number <= 7; number += 1)
      run(
        'register',
        '--role',
        'worker',
        '--id',
        `synthetic-worker-${number}`,
        '--session',
        `worker-${number}-1`,
        '--route',
        'human',
        '--lease-seconds',
        '3600',
      );
    await page.routeWebSocket(/.*/, (route) => {
      void route.close();
    });
    await page.setViewportSize({ width: 1440, height: 844 });
    await openProject(page, server);
    await page.getByRole('button', { name: 'Workers' }).click();
    const columns = page.locator('.worker-columns');
    await expect(columns.locator('[data-worker-id]')).toHaveCount(8, { timeout: 30_000 });
    await expect(columns).toContainText(workerId);
    await columns.locator('[data-ticket-qualified-id]').first().click();
    const inspector = page.getByRole('complementary', { name: 'Inspector rail' });
    await expect(inspector).toContainText('Synthetic ticket with a long but readable name');
    await page.getByRole('button', { name: 'Show Terminal Drawer' }).click();
    await expect(page.locator('[data-component="terminal-drawer"]')).toBeVisible();
    await inspector.getByRole('button', { name: 'Edit Ticket details' }).click();
    const editor = inspector.getByRole('textbox', { name: 'Ticket details' });
    await editor.fill('Unsaved synthetic worker edit');
    await expect(editor).toBeFocused();
    run(
      'register',
      '--role',
      'worker',
      '--id',
      'synthetic-worker-late',
      '--session',
      'worker-late-1',
      '--route',
      'human',
      '--lease-seconds',
      '3600',
    );
    await expect(columns.locator('[data-worker-id]')).toHaveCount(9, { timeout: 30_000 });
    await expect(editor).toHaveValue('Unsaved synthetic worker edit');
    await expect(editor).toBeFocused();
    for (const scheme of ['light', 'dark'] as const) {
      await page.emulateMedia({ colorScheme: scheme, reducedMotion: 'reduce' });
      for (const [name, width] of [
        ['wide', 1440],
        ['medium', 900],
        ['phone', 390],
        ['returned-wide', 1440],
      ] as const) {
        await page.setViewportSize({ width, height: 844 });
        await expect(columns.locator('[data-worker-id]')).toHaveCount(9);
        await page.evaluate(
          () =>
            new Promise<void>((resolve) => {
              requestAnimationFrame(() => {
                requestAnimationFrame(() => {
                  resolve();
                });
              });
            }),
        );
        const showInspector = page.getByRole('button', { name: 'Show Ticket Inspector' });
        if (await showInspector.isVisible()) await showInspector.click();
        await expect(page.getByRole('button', { name: 'Hide Ticket Inspector' })).toBeVisible();
        const detailsButton = inspector.getByRole('button', { name: 'Edit Ticket details' });
        // Collapsing the rail at a narrower width blurs the editor, which saves the draft and swaps the
        // textbox for the preview button. Read whichever one is present in a single bounded step: a
        // count followed by an unbounded inputValue() hung on the removed textbox (HS2-3JPQZH, HS2-DE8BDH).
        await expect
          .poll(() =>
            editor
              .or(detailsButton)
              .evaluate((node) => (node instanceof HTMLTextAreaElement ? node.value : node.textContent), undefined, {
                timeout: 1_000,
              })
              .catch(() => ''),
          )
          .toContain('Unsaved synthetic worker edit');
        await expect(inspector.getByRole('combobox', { name: 'Category' })).toBeVisible();
        await expect(inspector.getByRole('combobox', { name: 'Priority' })).toBeVisible();
        await page.screenshot({
          path: test.info().outputPath(`hs2-hwmw0d-${scheme}-${name}.png`),
          animations: 'disabled',
        });
      }
    }
    // The same save can still be removing the textbox; never wait unbounded on it (HS2-3JPQZH).
    if (await editor.count()) await editor.blur({ timeout: 1_000 }).catch(() => undefined);
    await page.getByRole('button', { name: 'Hide Ticket Inspector' }).click();
    await page.getByRole('button', { name: 'Hide Terminal Drawer' }).click();
    const longWorker = columns.getByRole('region', { name: `${workerId} Worker Column` });
    for (const scheme of ['light', 'dark'] as const) {
      await page.emulateMedia({ colorScheme: scheme, reducedMotion: 'reduce' });
      for (const [name, width] of [
        ['wide', 1440],
        ['medium', 900],
        ['phone', 390],
        ['returned-wide', 1440],
      ] as const) {
        await page.setViewportSize({ width, height: 844 });
        await longWorker.scrollIntoViewIfNeeded();
        await expect(longWorker).toBeInViewport();
        await expect(longWorker).toContainText('Synthetic ticket with a long but readable name');
        await page.screenshot({
          path: test.info().outputPath(`hs2-hwmw0d-workers-${scheme}-${name}.png`),
          animations: 'disabled',
        });
      }
    }
  } finally {
    await page.goto('about:blank').catch(() => undefined);
    await page.unrouteAll({ behavior: 'ignoreErrors' });
    await server.stop();
  }
});

test('HS2-HWMW0D shows a genuine zero-worker state in the real app', async ({ page }) => {
  test.setTimeout(120_000);
  let fixtureCli = '';
  let fixtureEnv: NodeJS.ProcessEnv = {};
  const server = await realTicketServer({
    seed: async ({ root, env, cli }) => {
      fixtureCli = cli;
      fixtureEnv = env;
      const initialized = spawnSync('git', ['-C', root, 'init'], {
        env: withoutGitRepositoryEnv(env),
        encoding: 'utf8',
      });
      expect(initialized.status, initialized.stderr).toBe(0);
      const committed = spawnSync(
        'git',
        [
          '-C',
          root,
          '-c',
          'user.name=Hot Sheet fixture',
          '-c',
          'user.email=fixture@example.invalid',
          'commit',
          '--allow-empty',
          '-m',
          'Seed synthetic checkout',
        ],
        { env: withoutGitRepositoryEnv(env), encoding: 'utf8' },
      );
      expect(committed.status, committed.stderr).toBe(0);
    },
  });
  try {
    const setup = spawnSync(fixtureCli, ['-C', server.store, 'coordination', '--project', server.root, 'setup'], {
      cwd: server.root,
      env: fixtureEnv,
      encoding: 'utf8',
    });
    expect(setup.status, setup.stderr).toBe(0);
    await page.routeWebSocket(/.*/, (route) => {
      void route.close();
    });
    await page.setViewportSize({ width: 1440, height: 844 });
    await openProject(page, server);
    await page.getByRole('button', { name: 'Workers' }).click();
    const columns = page.locator('.worker-columns');
    await expect(columns.locator('[data-worker-id]')).toHaveCount(0, { timeout: 30_000 });
    await expect(columns.locator('.worker-columns__zero')).toContainText('No workers are registered');
    for (const scheme of ['light', 'dark'] as const) {
      await page.emulateMedia({ colorScheme: scheme, reducedMotion: 'reduce' });
      for (const [name, width] of [
        ['wide', 1440],
        ['medium', 900],
        ['phone', 390],
        ['returned-wide', 1440],
      ] as const) {
        await page.setViewportSize({ width, height: 844 });
        await expect(columns.locator('.worker-columns__zero')).toContainText('No workers are registered');
        await page.screenshot({
          path: test.info().outputPath(`hs2-hwmw0d-zero-${scheme}-${name}.png`),
          animations: 'disabled',
        });
      }
    }
  } finally {
    await page.goto('about:blank').catch(() => undefined);
    await page.unrouteAll({ behavior: 'ignoreErrors' });
    await server.stop();
  }
});

// HS2-PPZQX9: a project whose one store stalled while another linked store progressed seconds ago used to
// read `Stalled · last progress <1m ago`. The state word now pairs with the progress of the stores that
// show it, in the chip title, the toggle's accessible name, and the expanded state row, while the Workers
// choice still names the project's freshest progress.
test('HS2-PPZQX9 pairs Stalled with the stalled store last progress, not a busier store', async ({ page }) => {
  test.setTimeout(120_000);
  const server = await realTicketServer();
  try {
    await page.routeWebSocket(/.*/, (route) => {
      void route.close();
    });
    await page.setViewportSize({ width: 1280, height: 900 });
    const now = Date.now(),
      MINUTE = 60_000;
    const stalledDemo = demoLiveState('stalled'),
      busyDemo = demoLiveState('quiet');
    const stalled = {
      ...stalledDemo,
      store: 'ppzqx9-stalled',
      checkouts: [server.checkoutId],
      claims: [],
      workers: { live: 0, expected: 1 },
      activity: { ...stalledDemo.activity, active_since_ms: null, last_progress_ms: now - 34 * MINUTE },
    };
    const busy = {
      ...busyDemo,
      store: 'ppzqx9-busy',
      checkouts: [server.checkoutId],
      activity: { ...busyDemo.activity, active_since_ms: now - 10 * MINUTE, last_progress_ms: now - 20_000 },
      claims: busyDemo.claims.map((claim) => ({
        ...claim,
        claimed_at_ms: now - 10 * MINUTE,
        lease_expires_at_ms: now + 20 * MINUTE,
        eta_at_ms: null,
      })),
    };
    await openProject(page, server, () =>
      page.route('**/live-state', (route) => route.fulfill({ json: { epoch_ms: 1, stores: [stalled, busy] } })),
    );
    const sidebar = page.locator('section[aria-label="Project sidebar"]').first();
    const live = sidebar.locator('[data-component="project-live-section"]');
    await expect(live).toHaveAttribute('data-mode', 'compact', { timeout: 30_000 });
    const chip = live.locator('[data-live-chip="state"]');
    await expect(chip).toHaveAttribute('title', 'Stalled · last progress 34m ago', { timeout: 30_000 });
    await expect(chip).toHaveAttribute('data-tone', 'danger');
    const toggle = live.getByRole('button', { name: /^Show live details\. / });
    await expect(toggle).toHaveAccessibleName('Show live details. Stalled · 2 workers live, last progress 34m ago');
    // The Workers choice names the project's freshest progress, from the busy store.
    const workers = page.locator('[data-segment-value="workers"]').first();
    await expect(workers).toHaveAttribute('title', '2 workers live · last progress <1m ago');
    await page.mouse.move(640, 880);
    for (const width of [1280, 1024, 1280]) {
      await page.setViewportSize({ width, height: 900 });
      await expect(chip).toBeVisible();
      await live.screenshot({ path: `target/visual-captures/hs2-ppzqx9-stalled-chip-${width}.png` });
    }

    await toggle.click();
    await expect(live).toHaveAttribute('data-mode', 'expanded');
    const row = live.locator('[data-live-row="state"]');
    await expect(row).toContainText('Stalled · 2 workers live');
    await expect(row).toContainText('34m ago');
    await page.mouse.move(640, 880);
    await live.screenshot({ path: 'target/visual-captures/hs2-ppzqx9-stalled-row-expanded-1280.png' });
  } finally {
    await server.stop(page);
  }
});
