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

for (const width of [1280, 390]) {
  test(`shows busy, halted, and Hot Sheet connection states on terminal tabs at ${width}px (HS2-EV1XK3)`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: 700 });
    await page.goto('/ux-demo?component=terminal-drawer');
    const states = page.locator('[aria-label="Terminal tab states"] [data-component="terminal-drawer"]'),
      tab = (id: string) => states.locator(`[data-tab-kind="terminal"][data-terminal-id="${id}"]`);
    await expect(tab('connected').locator('[data-ai-connection="connected"] [data-lucide="plug"]')).toBeVisible();
    await expect(tab('connected').locator('.terminal-drawer__busy-dot')).toBeVisible();
    await expect(tab('missing').locator('[data-ai-connection="missing"] [data-lucide="unplug"]')).toBeVisible();
    await expect(tab('missing').locator('.terminal-drawer__ai-connection')).toHaveAttribute(
      'title',
      /Run \/hooks in Codex/,
    );
    await expect(tab('halted').locator('.terminal-drawer__halt [data-lucide="triangle-alert"]')).toBeVisible();
    // The status icons sit on the tab label's center line.
    for (const [id, name] of [
      ['connected', 'Connected'],
      ['missing', 'Not connected'],
    ] as const) {
      const [label, icon] = await Promise.all([
        tab(id).getByText(name, { exact: true }).boundingBox(),
        tab(id).locator('.terminal-drawer__ai-connection svg').boundingBox(),
      ]);
      expect(Math.abs(label!.y + label!.height / 2 - (icon!.y + icon!.height / 2))).toBeLessThanOrEqual(1.5);
    }
    await states.screenshot({ path: test.info().outputPath(`hs2-ev1xk3-tab-states-${width}.png`) });
  });
}
