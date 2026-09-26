import { expect, test } from '@playwright/test';

test('sort hover stays within one centered pill surface', async ({ page }) => {
  await page.setViewportSize({ width: 900, height: 500 });
  await page.goto('/ux-demo?component=workspace-header');

  const group = page.locator('.workspace-header__sort-group');
  const select = group.locator('.workspace-header__sort');
  const idleBackground = await group.evaluate((node) => getComputedStyle(node).backgroundColor);

  await select.hover();
  await expect(group).toHaveCSS('background-color', idleBackground);
  // Kerf's icon-only Select presentation owns the trigger width (HS2-06GDW3); it must stay
  // centered inside the single group surface rather than drawing its own offset pill.
  await expect(select).toHaveAttribute('data-selected-presentation', 'icon-only');
  const geometry = await group.evaluate((node) => {
    const outer = node.getBoundingClientRect(),
      inner = node.querySelector<HTMLElement>('.workspace-header__sort')!.getBoundingClientRect();
    return { left: inner.left - outer.left, right: outer.right - inner.right, inside: inner.width <= outer.width };
  });
  expect(geometry.inside).toBe(true);
  expect(Math.abs(geometry.left - geometry.right)).toBeLessThanOrEqual(1);
  await group.screenshot({ path: test.info().outputPath('hs2-0sphk7-sort-hover.png') });
});
