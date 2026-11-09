import { spawnSync } from 'node:child_process';

import { expect, type Page, test } from '@playwright/test';

import { withoutGitRepositoryEnv } from '../scripts/repository-env.mjs';
import { realTicketServer } from './real-ticket-server';

type Server = Awaited<ReturnType<typeof realTicketServer>>;

/** Open the real app on an isolated server through the dev proxy, as archive-load-more does. */
async function openProject(page: Page, server: Server) {
  const apiPath = `/__hotsheet/project-api/${server.checkoutId}`;
  await page.route('**/__hotsheet/projects/open', (route) =>
    route.fulfill({
      status: 201,
      json: { id: server.checkoutId, root: server.root, name: 'live', stores: [server.store], apiPath },
    }),
  );
  await page.route(`**${apiPath}/**`, async (route) => {
    const incoming = new URL(route.request().url()),
      path = incoming.pathname.slice(apiPath.length);
    try {
      const response = await route.fetch({
        url: `${server.url}${path}${incoming.search}`,
        headers: { ...route.request().headers(), 'X-Hotsheet-Secret': server.secret },
        timeout: 60_000,
      });
      await route.fulfill({ response });
    } catch {
      await route.abort().catch(() => undefined);
    }
  });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open Project' }).click();
  await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();
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

    // Compact: two bare chip gauges named by their titles; the runway chip says what it means only
    // in its title and accessible name, and its face is the raw number.
    const gauges = live.locator('svg.glance-gauge');
    await expect(gauges).toHaveCount(2);
    await expect(gauges.nth(0)).toHaveAttribute('data-variant', 'bare');
    await expect(gauges.nth(0)).toHaveAccessibleName(/^Token rate: 0 tokens\/min/);
    await expect(gauges.nth(1)).toHaveAccessibleName(/^Tickets updated: \d+ tickets\/h/);
    const runway = live.locator('[data-live-chip="runway"]');
    await expect(runway).toHaveText('1');
    await expect(runway).toHaveAttribute('title', /^Queue runway.* · 1 ready$/);
    await expect(live.locator('[data-live-chip="waiting"], [data-live-chip="overrun"]')).toHaveCount(0);
    await expect(live.locator('[data-live-row]')).toHaveCount(0);

    // The header action switches to the detail rows and names the way back.
    await live.getByRole('button', { name: 'Show live details' }).click();
    await expect(live).toHaveAttribute('data-mode', 'expanded');
    await expect(live.locator('svg.glance-gauge[data-variant="full"]')).toHaveCount(2);
    await expect(live.locator('[data-live-row="state"]')).toBeVisible();
    await expect(live.locator('[data-live-row="runway"]')).toContainText('1 ready');
    await expect(live.locator('[data-live-row="waiting"]')).toHaveCount(0);
    await page.screenshot({ path: 'target/visual-captures/hs2-f788gw-live-expanded-1280.png' });

    // The choice persists per viewer across a reload, which restores the open project.
    await page.reload();
    await expect(live).toHaveAttribute('data-mode', 'expanded', { timeout: 30_000 });
    await live.getByRole('button', { name: 'Show compact live summary' }).click();
    await expect(live).toHaveAttribute('data-mode', 'compact');
    await expect(live.locator('[data-live-row]')).toHaveCount(0);
    await page.screenshot({ path: 'target/visual-captures/hs2-f788gw-live-compact-1280.png' });
  } finally {
    await server.stop();
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
    await server.stop();
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
        await expect
          .poll(async () =>
            (await editor.count())
              ? await editor.inputValue()
              : (await detailsButton.count())
                ? await detailsButton.textContent()
                : '',
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
    if (await editor.count()) await editor.blur();
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
