import { expect, test } from '@playwright/test';

test('sort hover stays within one centered pill surface', async ({ page }) => {
  await page.setViewportSize({ width: 900, height: 500 });
  await page.goto('/ux-demo?component=workspace-header');

  const group = page.locator('.workspace-header__sort-group');
  const select = group.locator('.workspace-header__sort');
  const comboboxBackground = () =>
    select.evaluate((node) => {
      const combobox = node.shadowRoot?.querySelector('[part~="combobox"]');
      return combobox ? getComputedStyle(combobox).backgroundColor : 'missing';
    });

  await select.hover();
  // Kerf 5.0.0-beta.51 lights the whole single-control group as one hover surface; the Select's own
  // combobox must stay transparent so it never draws a second, offset pill inside it (HS2-KMDJRH).
  await expect.poll(comboboxBackground).toBe('rgba(0, 0, 0, 0)');
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
