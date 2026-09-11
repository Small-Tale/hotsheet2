import { expect, test } from '@playwright/test';

test('adds tags from the shared Tags menu header at wide and narrow sizes (HS2-9ZPYR8)', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto('/ux-demo?component=ticket-inspector');

  const inspector = page.locator('[data-component="ticket-inspector"]');
  const trigger = inspector.getByRole('button', { name: 'Add tag' });
  const popover = page.getByRole('dialog', { name: 'Add tag' });
  const input = popover.getByRole('combobox', { name: 'Tag name' });
  const header = inspector.locator('[data-component="menu-header"]').filter({ hasText: 'Tags' });

  await expect(header).toHaveCount(1);
  await expect(trigger.locator('[data-lucide="plus"]')).toBeVisible();
  await expect(trigger).toHaveAttribute('aria-haspopup', 'dialog');
  await expect(popover).toBeHidden();
  await trigger.click();
  await expect(popover).toBeVisible();
  await expect(input).toBeFocused();
  expect(await popover.locator('option').evaluateAll(options => options.map(option => (option as HTMLOptionElement).value))).toEqual(['accessibility', 'regression', 'server']);

  const wideLayout = await page.evaluate(() => {
    const triggerBox = document.querySelector<HTMLElement>('[data-component="menu-header"] button[aria-label="Add tag"]')!.getBoundingClientRect();
    const popoverBox = document.querySelector<HTMLElement>('[data-component="ticket-tag-popover"]')!.getBoundingClientRect();
    return {
      belowTrigger: popoverBox.top >= triggerBox.bottom,
      withinViewport: popoverBox.left >= 0 && popoverBox.right <= innerWidth && popoverBox.bottom <= innerHeight,
    };
  });
  expect(wideLayout).toEqual({ belowTrigger: true, withinViewport: true });
  await page.screenshot({ path: '/private/tmp/hs2-9zpyr8-tags-menu-header-wide.png', fullPage: true });

  await page.keyboard.press('Escape');
  await expect(popover).toBeHidden();
  await expect(trigger).toBeFocused();
  await page.setViewportSize({ width: 760, height: 640 });
  await trigger.click();
  await expect(input).toBeFocused();
  await page.screenshot({ path: '/private/tmp/hs2-9zpyr8-tags-menu-header-narrow.png', fullPage: true });
  await expect.poll(() => page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);

  await input.fill('regression');
  await input.press('Enter');
  await expect(inspector.locator('[data-component="tag-chip"][data-tag-id="regression"]')).toBeVisible();
  await expect(popover).toBeVisible();
  await expect(input).toHaveValue('');
  await page.keyboard.press('Escape');
  await expect(popover).toBeHidden();
});
