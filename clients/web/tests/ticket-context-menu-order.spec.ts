import { mkdir } from 'node:fs/promises';

import { expect, test } from '@playwright/test';

import { openDemoProject } from './mobile-project-fixture';

test('HS2-R3ZS3V keeps Toggle Up Next at the top of the real ticket menu across widths', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await openDemoProject(page);
  const row = page.locator('[data-action="select-ticket-row"][data-ticket-slug="HS2-M1"]').first(),
    menu = page.locator('[data-context-menu="ticket"]'),
    upNext = menu.locator('[data-context-action="Toggle Up Next"]'),
    captureDir = process.env.HS2_R3ZS3V_CAPTURE_DIR;
  for (const [name, width] of [
    ['wide', 1280],
    ['medium', 940],
    ['narrow', 390],
    ['returned-wide', 1280],
  ] as const) {
    await page.setViewportSize({ width, height: 800 });
    await row.scrollIntoViewIfNeeded();
    await row.click({ button: 'right' });
    await expect(upNext).toBeVisible();
    await expect(menu.locator('[data-context-action]').first()).toHaveAttribute(
      'data-context-action',
      'Toggle Up Next',
    );
    await expect(menu.locator('[data-context-action="Open ticket"]')).toBeVisible();
    await expect(menu.locator('[data-context-action="Copy ticket IDs"]')).toBeVisible();
    const bounds = await upNext.boundingBox();
    expect(bounds).not.toBeNull();
    expect(bounds!.x).toBeGreaterThanOrEqual(0);
    expect(bounds!.x + bounds!.width).toBeLessThanOrEqual(width);
    if (captureDir) {
      await mkdir(captureDir, { recursive: true });
      await page.screenshot({ path: `${captureDir}/hs2-r3zs3v-${name}.png` });
    }
    await page.keyboard.press('Escape');
    await expect(menu).toHaveCount(0);
  }
});
