import { expect, test } from '@playwright/test';

test('sort hover stays within one centered pill surface', async ({ page }) => {
  await page.setViewportSize({ width: 900, height: 500 });
  await page.goto('/ux-demo?component=workspace-header');

  const group = page.locator('.workspace-header__sort-group');
  const select = group.locator('.workspace-header__sort');
  const idleBackground = await group.evaluate(node => getComputedStyle(node).backgroundColor);

  await select.hover();
  await expect(group).toHaveCSS('background-color', idleBackground);
  await expect(select).toHaveCSS('width', '44px');
  await group.screenshot({ path: '/private/tmp/hs2-0sphk7-sort-hover.png' });
});
