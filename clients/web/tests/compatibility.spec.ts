import { fileURLToPath } from 'node:url';

import { expect, test } from '@playwright/test';

import type { CompatibilityAssessment } from '../src/compatibility';
import { releaseBinaryEnvironment } from '../src/local-host';
import { ticketsJson } from './ticket-page-fixture';

test('selects current local server, CLI, and migrator binaries from the real checkout', () => {
  const root = fileURLToPath(new URL('../../../', import.meta.url));
  const selected = releaseBinaryEnvironment(root, {});
  expect(selected.HOTSHEET_SERVER_BIN).toMatch(/\/target\/(?:debug|release)\/hotsheet-server$/);
  expect(selected.HOTSHEET_CLI_BIN).toMatch(/\/target\/(?:debug|release)\/hotsheet-cli$/);
  expect(selected.HOTSHEET_MIGRATE_BIN).toMatch(/\/target\/(?:debug|release)\/hotsheet-migrate$/);
  expect(selected.HOT_SHEET_BUILD_REVISION).toMatch(/^source-sha256:[0-9a-f]{64}$/);
});

async function openWithCompatibility(
  page: import('@playwright/test').Page,
  compatibility: CompatibilityAssessment,
  project: Record<string, unknown> = {},
) {
  await page.route('**/*', (route) => {
    const path = new URL(route.request().url()).pathname;
    if (path === '/__hotsheet/projects/open')
      return route.fulfill({
        status: 201,
        json: {
          id: 'skew',
          root: '/work/skew',
          name: 'skew',
          stores: ['/work/skew.hs2'],
          apiPath: '/__hotsheet/project-api/skew',
          compatibility,
          ...project,
        },
      });
    if (path.endsWith('/providers')) return route.fulfill({ json: [] });
    if (path.endsWith('/tickets')) return route.fulfill({ json: ticketsJson(route.request(), []) });
    if (path.endsWith('/repository/status'))
      return route.fulfill({ json: { ahead: 0, behind: 0, staged: 0, unstaged: 0, untracked: 0, conflicted: 0 } });
    return route.continue();
  });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open Project' }).click();
  await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();
}

test('does not offer unsafe restart for an old server', async ({ page }) => {
  await openWithCompatibility(page, {
    kind: 'server_too_old',
    detail: 'Server protocol 0–0 is older.',
    revisionMismatch: false,
    sourceStale: false,
    canRestartServer: false,
  });
  const banner = page.locator('.connection-state-banner');
  await expect(banner).toContainText('Server update required');
  await expect(banner).toContainText('Safe restart is unavailable');
  await expect(banner.getByRole('button', { name: /Restart/ })).toHaveCount(0);
});

test('offers reload when the client is too old', async ({ page }) => {
  await openWithCompatibility(page, {
    kind: 'client_too_old',
    detail: 'Client protocol 1–1 is older.',
    revisionMismatch: false,
    sourceStale: false,
    canRestartServer: false,
  });
  await expect(page.locator('.connection-state-banner')).toContainText('Client update required');
  await expect(page.getByRole('button', { name: 'Reload client' })).toBeVisible();
});

test('surfaces unavailable compatibility metadata without blocking project data', async ({ page }) => {
  await openWithCompatibility(page, {
    kind: 'unknown',
    detail: 'The server did not provide compatibility metadata.',
    revisionMismatch: false,
    sourceStale: false,
    canRestartServer: false,
  });
  await expect(page.locator('.connection-state-banner')).toContainText('Server compatibility unknown');
  await expect(page.getByRole('heading', { name: 'Queue', level: 1 })).toBeVisible();
});

test('surfaces a compatible detached server from another development revision', async ({ page }) => {
  await openWithCompatibility(page, {
    kind: 'compatible',
    revisionMismatch: true,
    sourceStale: false,
    canRestartServer: false,
  });
  const banner = page.locator('.connection-state-banner');
  await expect(banner).toContainText('Different server build is running');
  await expect(banner).toContainText('protocol is compatible');
});

test('keeps a pinned production build connected after workspace source changes', async ({ page }) => {
  await openWithCompatibility(page, {
    kind: 'compatible',
    revisionMismatch: false,
    sourceStale: true,
    canRestartServer: false,
  });
  await expect(page.getByRole('heading', { name: 'Queue', level: 1 })).toBeVisible();
  await expect(page.locator('.connection-state-banner')).toHaveCount(0);
});

test('tells development users to restart a server built from older local source', async ({ page }) => {
  await openWithCompatibility(page, {
    kind: 'compatible',
    detail: 'The running server build differs from this checkout.',
    revisionMismatch: true,
    sourceStale: true,
    canRestartServer: false,
    clientProtocol: { min: 1, max: 1 },
    clientRevision: 'source-sha256:client',
    server: {
      generation: 'hs2',
      application_version: '0.1.0',
      build_revision: 'source-sha256:old',
      source_revision: 'source-sha256:current',
      source_stale: true,
      protocol: { min: 1, max: 1 },
      started_at: '2026-09-02T08:00:00Z',
    },
  });
  const banner = page.locator('.connection-state-banner');
  await expect(banner).toContainText('Different server build is running');
  await expect(banner).toContainText('Rebuild if needed, then restart it to pick up your latest build');
  await expect(page.getByRole('heading', { name: 'Queue', level: 1 })).toBeVisible();
  await banner.getByRole('button', { name: 'View details' }).click();
  const dialog = page.getByRole('dialog', { name: 'Server Build Details' });
  await expect(dialog).toBeVisible();
  await expect(dialog.getByRole('term')).toHaveText([
    'Running server version',
    'Running server build',
    'Current server source',
    'Client build',
    'Protocol ranges',
    'Server started',
  ]);
  await expect(dialog).toContainText('0.1.0');
  await expect(dialog).toContainText('source-sha256:old');
  await expect(dialog).toContainText('source-sha256:current');
  await expect(dialog).toContainText('source-sha256:client');
  await expect(dialog).toContainText('Client 1–1 · Server 1–1');
  await expect(dialog).toContainText('cargo build -p hotsheet-server');
  await expect(dialog.locator('[data-component="toolbar"]:has(.kui-toolbar-text__description)')).toBeVisible();
  await expect(dialog.locator('[data-component="value-table"]')).toBeVisible();
  await expect(dialog.getByRole('button', { name: 'Close' })).toHaveCount(0);
  await page.screenshot({ path: 'target/visual-captures/hs2-xrs9s4-server-details-wide.png', fullPage: true });
  await page.setViewportSize({ width: 390, height: 844 });
  await page.screenshot({ path: 'target/visual-captures/hs2-xrs9s4-server-details-narrow.png', fullPage: true });
  await page.mouse.click(4, 4);
  await expect(dialog).not.toBeVisible();
});

const staleOnly = (build: string, source: string): CompatibilityAssessment => ({
  kind: 'compatible',
  revisionMismatch: true,
  sourceStale: true,
  canRestartServer: false,
  clientProtocol: { min: 1, max: 1 },
  server: {
    generation: 'hs2',
    build_revision: build,
    source_revision: source,
    source_stale: true,
    protocol: { min: 1, max: 1 },
  },
});

test('presents newer local source as a globally dismissible info notice, not an alert (HS2-B84P1S)', async ({
  page,
}) => {
  let current = { compatibility: staleOnly('source-sha256:old', 'source-sha256:new'), id: 'first' };
  await openWithCompatibility(page, current.compatibility);
  // Registered after the helper's catch-all so reloads see each step's project and build pair.
  await page.route('**/__hotsheet/projects/open', (route) =>
    route.fulfill({
      status: 201,
      json: {
        id: current.id,
        root: `/work/${current.id}`,
        name: current.id,
        stores: [`/work/${current.id}.hs2`],
        apiPath: `/__hotsheet/project-api/${current.id}`,
        compatibility: current.compatibility,
      },
    }),
  );
  const notice = page.locator('[data-component="source-stale-notice-banner"]');
  await expect(page.locator('[data-project-dialog]')).toBeHidden();
  for (const width of [1280, 390]) {
    await page.setViewportSize({ width, height: 844 });
    await expect(notice).toContainText('Newer local source is available');
    await expect(notice).toHaveAttribute('data-tone', 'info');
    expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(width);
    // Let the shell settle into this width's layout before the visual capture.
    await page.waitForTimeout(1_000);
    await page.screenshot({
      path: `target/visual-captures/hs2-b84p1s-source-stale-notice-${width}.png`,
      animations: 'disabled',
    });
  }
  await page.setViewportSize({ width: 1280, height: 844 });
  // The red "different build" alert no longer appears for this case.
  await expect(page.locator('.connection-state-banner')).toHaveCount(0);
  await notice.getByRole('button', { name: 'Dismiss' }).click();
  await expect(notice).toHaveCount(0);
  // Another project on the same pair stays quiet: the dismissal is global.
  current = { ...current, id: 'second' };
  await page.reload();
  await expect(page.getByRole('heading', { name: 'Queue', level: 1 })).toBeVisible();
  await expect(notice).toHaveCount(0);
  // The source moves again: the new pair is announced.
  current = { ...current, compatibility: staleOnly('source-sha256:old', 'source-sha256:newer') };
  await page.reload();
  await expect(notice).toContainText('Newer local source is available');
});

test('wraps stacked alert and shell banners inside a phone-width viewport (HS2-TM2D7J)', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await openWithCompatibility(
    page,
    {
      kind: 'server_too_old',
      detail: 'Server protocol 0–0 is older than client protocol 1–1.',
      revisionMismatch: false,
      sourceStale: false,
      canRestartServer: false,
    },
    {
      setupWarning:
        'The development Hot Sheet CLI does not match the current setup templates and may overwrite newer project guidance. Run cargo build -p hotsheet-cli, then reopen the project.',
    },
  );
  const alert = page.locator('.connection-state-banner'),
    warning = page.locator('[data-component="project-setup-warning-banner"]');
  await expect(alert).toContainText('Server update required');
  await expect(warning).toContainText('Project setup needs attention');
  const overflow = await page.evaluate(() =>
    [...document.querySelectorAll('.app-shell__banner > *')].map((banner) => {
      const box = banner.getBoundingClientRect();
      return { right: box.right, scroll: banner.scrollWidth - banner.clientWidth };
    }),
  );
  for (const banner of overflow) {
    expect(banner.right).toBeLessThanOrEqual(390);
    expect(banner.scroll).toBeLessThanOrEqual(0);
  }
  await expect(alert.getByRole('button', { name: 'View details' })).toBeInViewport();
  // The detail wraps on its own line rather than truncating beside the title.
  expect(
    await alert.evaluate((banner) =>
      [...banner.querySelectorAll('*')].some((node) => node.scrollWidth > node.clientWidth + 1),
    ),
  ).toBe(false);
  await expect(page.locator('[data-project-dialog]')).toBeHidden();
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(390);
  await page.waitForTimeout(1_000);
  await page.screenshot({ path: 'target/visual-captures/hs2-tm2d7j-stacked-banners-390.png', animations: 'disabled' });
  await page.setViewportSize({ width: 1280, height: 844 });
  await page.waitForTimeout(1_000);
  await page.screenshot({ path: 'target/visual-captures/hs2-tm2d7j-stacked-banners-1280.png', animations: 'disabled' });
});
