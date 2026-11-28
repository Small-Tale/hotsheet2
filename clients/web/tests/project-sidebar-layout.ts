import { expect, type Page } from '@playwright/test';

import { isMobileViewport } from '../src/mobile-layout';

/** HS2-P36R6N: resize delivery, the desktop restore RAF, and CSS motion precede a resting rail.
 * An empty getAnimations() result alone can arrive before any of those have started. */
export async function expectProjectSidebarLayout(page: Page, expanded: boolean) {
  const rail = page.locator('#app-left-rail'),
    content = rail.locator(':scope > .kui-workbench__panel-content');
  await expect(rail).toHaveAttribute('data-collapsed', String(!expanded));
  if (expanded) {
    await expect(content).toHaveCSS('transform', 'none');
    await expect.poll(async () => Math.round((await content.boundingBox())!.x)).toBe(0);
  } else {
    await expect
      .poll(async () => {
        const bounds = (await content.boundingBox())!;
        return Math.round(bounds.x + bounds.width);
      })
      .toBe(0);
  }
  expect(await page.evaluate(() => window.scrollX)).toBe(0);
}

/**
 * HS2-Y32YFS: resize to `width` and show the project sidebar there. The app moves between its inline
 * rail and its mobile drawer only once the resize reaches it, so a check made straight after
 * `setViewportSize` can still see the previous layout; under load that skipped the drawer's toggle and
 * left the sidebar off screen. Wait for the shell's own layout first, then open the drawer when the
 * width is a mobile one.
 */
export async function showProjectSidebarAt(page: Page, width: number, height = 900) {
  await page.setViewportSize({ width, height });
  const mobile = isMobileViewport(width),
    rail = page.locator('#app-left-rail');
  await expect(page.locator('[data-component="app-shell"]')).toHaveAttribute('data-mobile', String(mobile));
  if (mobile && (await rail.getAttribute('data-collapsed')) === 'true')
    await page.getByRole('button', { name: 'Show Project Sidebar', exact: true }).click();
  await expect(rail).toHaveAttribute('data-collapsed', 'false');
}
