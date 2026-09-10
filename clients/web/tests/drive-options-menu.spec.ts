import { expect, test } from '@playwright/test';

test('uses one disclosure and aligned icon-label choices at wide and narrow sizes', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto('/ux-demo?component=drive-options-menu');

  const menu = page.locator('[data-component="drive-options-menu"]');
  const provider = menu.locator('wa-dropdown > wa-dropdown-item').filter({ hasText: 'Provider' });
  const model = menu.locator('wa-dropdown > wa-dropdown-item').filter({ hasText: 'Model' });
  const effort = menu.locator('wa-dropdown > wa-dropdown-item').filter({ hasText: 'Effort' });

  await expect(provider).toBeVisible();
  await expect(provider.locator(':scope > [data-lucide="chevron-right"]')).toHaveCount(0);
  expect(await provider.evaluate(node => node.shadowRoot?.querySelectorAll('[part="submenu-icon"]').length)).toBe(1);

  await model.hover();
  const modelChoices = model.locator(':scope > wa-dropdown-item[slot="submenu"]');
  await expect(modelChoices.first()).toBeVisible();
  await expect(modelChoices.first()).toHaveAttribute('aria-current', 'true');
  const modelGeometry = await modelChoices.first().evaluate(node => {
    const item = node.getBoundingClientRect();
    const icon = node.querySelector('[slot="icon"]')!.getBoundingClientRect();
    const label = node.shadowRoot!.querySelector('[part="label"]')!.getBoundingClientRect();
    return { iconInset: icon.left - item.left, labelInset: label.left - item.left };
  });
  await page.screenshot({ path: '/private/tmp/hs2-02hj6c-drive-menu-wide.png', fullPage: true });

  await effort.hover();
  const effortChoices = effort.locator(':scope > wa-dropdown-item[slot="submenu"]');
  await expect(effortChoices).toHaveCount(3);
  await expect(effort.locator(':scope > wa-dropdown-item[slot="submenu"][data-value="high"]')).toHaveAttribute('aria-current', 'true');
  const effortGeometry = await effortChoices.first().evaluate(node => {
    const item = node.getBoundingClientRect();
    const icon = node.querySelector('[slot="icon"]')!.getBoundingClientRect();
    const label = node.shadowRoot!.querySelector('[part="label"]')!.getBoundingClientRect();
    return { iconInset: icon.left - item.left, labelInset: label.left - item.left };
  });
  expect(effortGeometry.iconInset).toBeCloseTo(modelGeometry.iconInset, 0);
  expect(Math.abs(effortGeometry.labelInset-modelGeometry.labelInset)).toBeLessThan(2);

  await page.setViewportSize({ width: 760, height: 640 });
  await effort.hover();
  await expect(effortChoices.first()).toBeVisible();
  await expect.poll(() => page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await page.screenshot({ path: '/private/tmp/hs2-02hj6c-drive-menu-narrow.png', fullPage: true });
});
