import { spawnSync } from 'node:child_process';

import { expect, type Page, test } from '@playwright/test';

import { withoutGitRepositoryEnv } from '../scripts/repository-env.mjs';
import { remoteProjectId } from '../src/remote-projects/identity';
import { demoHostHealth } from '../src/ux-demo/live-section-fixtures';
import { openMockedProject } from './open-mocked-project';
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
