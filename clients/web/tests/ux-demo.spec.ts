import { expect, test } from '@playwright/test';

test('navigates the catalog and preserves URL-addressable selection', async ({ page }) => {
  await page.goto('/ux-demo');
  await expect(page.getByRole('heading', { name: 'UX components' })).toBeVisible();
  await expect(page.getByRole('heading', { name: 'TagChip', exact: true })).toBeVisible();
  await page.getByRole('link', { name: /TicketRow/ }).click();
  await expect(page).toHaveURL('/ux-demo?component=ticket-row');
  await expect(page.getByRole('heading', { name: 'TicketRow', exact: true })).toBeVisible();
  await expect(page.getByRole('region', { name: 'TicketRow planned demo' })).toBeVisible();
  await page.goBack();
  await expect(page.getByRole('heading', { name: 'TagChip', exact: true })).toBeVisible();
});

test('adjusts and removes TagChip through its settings inspector', async ({ page }) => {
  await page.goto('/ux-demo?component=tag-chip');
  const chip = page.locator('[data-component="tag-chip"]');
  await expect(chip).toContainText('needs-design');
  await page.getByRole('button', { name: 'Settings' }).click();
  const drawer = page.locator('wa-drawer[data-settings-drawer]');
  await expect(drawer).toHaveJSProperty('open', true);
  await expect(drawer.locator('[part="dialog"]')).toBeVisible();
  await page.locator('wa-input[name="label"]').evaluate((node: HTMLElement & { value: string }) => {
    node.value = 'server'; node.dispatchEvent(new Event('input', { bubbles: true }));
  });
  await expect(chip).toContainText('server');
  await page.locator('wa-select[name="variant"]').evaluate((node: HTMLElement & { value: string }) => {
    node.value = 'success'; node.dispatchEvent(new Event('change', { bubbles: true }));
  });
  await expect(chip).toHaveAttribute('variant', 'success');
  await chip.locator('[part~="remove-button"]').click();
  await expect(page.getByText('Remove requested for demo-tag')).toBeVisible();
});
