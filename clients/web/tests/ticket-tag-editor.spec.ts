import { expect, test } from '@playwright/test';

test('adds tags through the Kerf Combobox in the shared Tags menu at wide and narrow sizes (HS2-C32ERK)', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto('/ux-demo?component=ticket-inspector');

  const inspector = page.locator('[data-component="ticket-inspector"]');
  const trigger = inspector.getByRole('button', { name: 'Add tag' });
  const popover = page.getByRole('dialog', { name: 'Add tag' });
  const input = popover.getByRole('combobox', { name: 'Tag Name' });
  const header = inspector.locator('[data-component="list-header"]').filter({ hasText: 'Tags' });

  await expect(header).toHaveCount(1);
  await expect(trigger.locator('[data-lucide="plus"]')).toBeVisible();
  await expect(trigger).toHaveAttribute('aria-haspopup', 'dialog');
  await expect(popover).toBeHidden();
  await trigger.click();
  await expect(popover).toBeVisible();
  await expect(input).toBeFocused();
  // The Combobox snapshots unused suggestions from the editor's datalist when the surface opens.
  const listbox = popover.getByRole('listbox');
  await expect(listbox).toBeVisible();
  expect(
    await listbox
      .locator('[role="option"]')
      .evaluateAll((options) => options.map((option) => option.getAttribute('data-value'))),
  ).toEqual(['accessibility', 'regression', 'server', '']);
  await expect(popover.locator('[data-component="ticket-tag-popover"]')).toBeVisible();

  const wideLayout = await page.evaluate(() => {
    const triggerBox = document
      .querySelector<HTMLElement>('[data-component="list-header"] button[aria-label="Add tag"]')!
      .getBoundingClientRect();
    const popoverBox = document
      .querySelector<HTMLElement>('[role="dialog"][aria-label="Add tag"]')!
      .getBoundingClientRect();
    return {
      belowTrigger: popoverBox.top >= triggerBox.bottom,
      withinViewport: popoverBox.left >= 0 && popoverBox.right <= innerWidth && popoverBox.bottom <= innerHeight,
    };
  });
  expect(wideLayout).toEqual({ belowTrigger: true, withinViewport: true });
  await page.screenshot({ path: 'target/visual-captures/hs2-9zpyr8-tags-menu-header-wide.png', fullPage: true });

  await page.keyboard.press('Escape');
  await expect(popover).toBeHidden();
  await expect(trigger).toBeFocused();
  await page.setViewportSize({ width: 390, height: 844 });
  await trigger.click();
  await expect(input).toBeFocused();
  await page.screenshot({ path: 'target/visual-captures/hs2-9zpyr8-tags-menu-header-narrow.png', fullPage: true });
  await expect.poll(() => page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);

  await input.fill('reg');
  await input.press('ArrowDown');
  await input.press('Enter');
  await expect(inspector.locator('.tag-chip[data-item-id="regression"]')).toBeVisible();
  await expect(popover).toBeVisible();
  await expect(input).toHaveValue('');
  await input.fill('new-tag');
  await input.press(',');
  await expect(inspector.locator('.tag-chip[data-item-id="new-tag"]')).toBeVisible();
  await expect(input).toHaveValue('');
  await page.keyboard.press('Escape');
  await expect(popover).toBeHidden();
  await trigger.click();
  await expect(listbox.locator('[data-value="regression"]')).toHaveCount(0);
  await input.fill('access');
  await listbox.getByRole('option', { name: 'accessibility' }).click();
  await expect(inspector.locator('.tag-chip[data-item-id="accessibility"]')).toBeVisible();
  await expect(input).toHaveValue('');
});
