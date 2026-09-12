import { expect, test } from '@playwright/test';

test('keeps selected terminal-tab shadows inside the horizontal scrollport', async ({ page }) => {
  await page.setViewportSize({ width: 900, height: 600 });
  await page.goto('/ux-demo?component=terminal-drawer');

  const drawer = page.locator('[data-component="terminal-drawer"]');
  const gridTab = drawer.getByRole('tab', { name: 'Project grid' });
  const tabs = drawer.locator('.terminal-drawer__tabs');
  await expect(drawer.locator('[data-component="terminal-tab"]')).toHaveCount(1);

  const shadowGutter = await tabs.evaluate(node => {
    const scroller = node.getBoundingClientRect();
    const selected = node.querySelector<HTMLElement>('.kui-app-tab[data-selected="true"]')!.getBoundingClientRect();
    return { above: selected.top - scroller.top, below: scroller.bottom - selected.bottom };
  });
  expect(shadowGutter.above).toBeGreaterThanOrEqual(2);
  expect(shadowGutter.below).toBeGreaterThanOrEqual(4);
  await tabs.screenshot({ path: '/private/tmp/hs2-fhqgjn-terminal-tab-shadow-after.png' });

  const gridWidth = (await gridTab.boundingBox())!.width;
  await drawer.evaluate(node => {
    node.style.width = '520px';
    const tabs = node.querySelector('.terminal-drawer__tabs')!;
    const source = tabs.firstElementChild!;
    for (let index = 0; index < 8; index += 1) tabs.append(source.cloneNode(true));
  });
  await expect(drawer.locator('[data-component="terminal-tab"]')).toHaveCount(9);
  expect((await gridTab.boundingBox())!.width).toBeCloseTo(gridWidth, 0);
  expect(await tabs.evaluate(node => node.scrollWidth)).toBeGreaterThan(await tabs.evaluate(node => node.clientWidth));
});
