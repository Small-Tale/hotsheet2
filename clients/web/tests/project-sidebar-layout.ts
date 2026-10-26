import { expect, type Page } from '@playwright/test';

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
