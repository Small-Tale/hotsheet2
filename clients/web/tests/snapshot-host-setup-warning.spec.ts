import { expect, test } from '@playwright/test';

import { requireCurrentSetupAssets } from '../src/project-bridge';
import { openDemoProject, project } from './mobile-project-fixture';

// HS2-A3C894: a snapshot LAN host whose pinned CLI predates in-tree template edits still skips the
// setup refresh, but tells the user to restart the host rather than rebuild the CLI.
const snapshotWarning = (() => {
  try {
    requireCurrentSetupAssets(
      {
        generation: 'test',
        setup_assets_fingerprint: '0'.repeat(64),
        store_schema: { min: 1, max: 1, creates: 1 },
      },
      'f'.repeat(64),
      true,
    );
  } catch (error) {
    return (error as Error).message;
  }
  throw new Error('a mismatched snapshot fingerprint must be refused');
})();

test('explains snapshot-host template drift as a restart in the setup banner (HS2-A3C894)', async ({ page }) => {
  await openDemoProject(page);
  await expect(page.locator('[data-component="app-shell"]')).toBeVisible();
  await page.route('**/__hotsheet/projects/open', (route) =>
    route.fulfill({
      status: 201,
      json: { ...project('demo-checkout', '/work/demo'), setupWarning: snapshotWarning },
    }),
  );
  for (const width of [1280, 390]) {
    await page.setViewportSize({ width, height: 800 });
    await page.reload();
    const banner = page.locator('[data-component="project-setup-warning-banner"]');
    await expect(banner).toContainText('Project setup needs attention');
    await expect(banner).toContainText('Setup templates changed after this Hot Sheet host started');
    await expect(banner).toContainText('Restart the host');
    await expect(banner).not.toContainText('cargo build');
    expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(width);
    await page.screenshot({
      path: process.env.HOTSHEET_SCREENSHOT_DIR
        ? `${process.env.HOTSHEET_SCREENSHOT_DIR}/snapshot-setup-warning-${width}.png`
        : test.info().outputPath(`snapshot-setup-warning-${width}.png`),
    });
    await banner.getByRole('button', { name: 'Dismiss' }).click();
    await expect(banner).toHaveCount(0);
  }
});
