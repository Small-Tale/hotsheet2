import { expect, test } from '@playwright/test';

test('keeps selected terminal-tab shadows inside the horizontal scrollport', async ({ page }) => {
  await page.setViewportSize({ width: 900, height: 600 });
  await page.goto('/ux-demo?component=terminal-drawer');

  const drawer = page.locator('.terminal-drawer-demo [data-component="terminal-drawer"]');
  const gridTab = drawer.getByRole('tab', { name: 'Project grid' });
  const tabs = drawer.locator('.kui-tab-bar__tabs');
  await expect(drawer.locator('[data-tab-kind="terminal"]')).toHaveCount(1);

  const shadowGutter = await tabs.evaluate((node) => {
    const scroller = node.getBoundingClientRect();
    const selected = node.querySelector<HTMLElement>('.kui-app-tab[data-selected="true"]')!.getBoundingClientRect();
    return { above: selected.top - scroller.top, below: scroller.bottom - selected.bottom };
  });
  expect(shadowGutter.above).toBeGreaterThanOrEqual(2);
  expect(shadowGutter.below).toBeGreaterThanOrEqual(4);
  await tabs.screenshot({ path: '/private/tmp/hs2-fhqgjn-terminal-tab-shadow-after.png' });
  await drawer.screenshot({ path: '/private/tmp/hs2-4y6sm9-terminal-drawer-wide.png' });

  const gridWidth = (await gridTab.boundingBox())!.width;
  await drawer.evaluate((node) => {
    node.style.width = '520px';
  });
  await drawer.screenshot({ path: '/private/tmp/hs2-4y6sm9-terminal-drawer-narrow.png' });
  const overflow = await drawer.evaluate((node) => {
    const tabs = node.querySelector('.kui-tab-bar__tabs')!;
    const source = tabs.querySelector('[data-tab-kind="terminal"]')!;
    for (let index = 0; index < 8; index += 1) tabs.append(source.cloneNode(true));
    return {
      count: tabs.querySelectorAll('[data-tab-kind="terminal"]').length,
      scrollWidth: tabs.scrollWidth,
      clientWidth: tabs.clientWidth,
    };
  });
  expect(overflow.count).toBe(9);
  expect(overflow.scrollWidth).toBeGreaterThan(overflow.clientWidth);
  expect((await gridTab.boundingBox())!.width).toBeCloseTo(gridWidth, 0);
});

test('keeps the hide-drawer action at the trailing edge of the drawer toolbar (HS2-RGF5NE)', async ({ page }) => {
  for (const width of [1280, 390]) {
    await page.setViewportSize({ width, height: 700 });
    await page.goto('/ux-demo?component=terminal-drawer');
    const drawer = page.locator('.terminal-drawer-demo [data-component="terminal-drawer"]'),
      rail = drawer.locator('.terminal-drawer__rail'),
      hide = drawer.getByRole('button', { name: 'Hide terminal drawer' }),
      create = drawer.locator('[data-terminal-drawer-create]');
    await expect(hide).toBeVisible();
    const [railBox, hideBox, createBox] = await Promise.all([
      rail.boundingBox(),
      hide.boundingBox(),
      create.boundingBox(),
    ]);
    // The hide action ends at the rail's trailing edge (within its inset), not beside the tabs.
    expect(railBox!.x + railBox!.width - (hideBox!.x + hideBox!.width)).toBeLessThanOrEqual(16);
    // The create action stays adjacent to the tabs, ahead of the hide action.
    expect(createBox!.x + createBox!.width).toBeLessThan(hideBox!.x);
    await rail.screenshot({ path: test.info().outputPath(`terminal-drawer-hide-trailing-${width}.png`) });
  }
});
