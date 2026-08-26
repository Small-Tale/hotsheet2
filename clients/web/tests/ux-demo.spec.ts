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
  await page.setViewportSize({ width: 1600, height: 900 });
  await page.goto('/ux-demo?component=tag-chip');
  const chip = page.locator('[data-component="tag-chip"]');
  await expect(chip).toContainText('needs-design');
  const toggle = page.locator('[data-action="toggle-settings"]');
  await expect(toggle).toHaveCount(1);
  await expect(toggle).toContainText('Settings');
  const closedToggleBox = await toggle.boundingBox();
  await toggle.click();
  await expect(toggle).toHaveCount(1);
  await expect(toggle).toContainText('Close settings');
  await expect(toggle).toHaveAttribute('aria-expanded', 'true');
  const openToggleBox = await toggle.boundingBox();
  expect(closedToggleBox).not.toBeNull();
  expect(openToggleBox).not.toBeNull();
  expect(Math.abs(openToggleBox!.x - closedToggleBox!.x)).toBeLessThanOrEqual(1);
  expect(Math.abs(openToggleBox!.y - closedToggleBox!.y)).toBeLessThanOrEqual(1);
  const inspector = page.getByRole('complementary', { name: 'TagChip settings' });
  await expect(inspector).toBeVisible();
  const [detailBox, inspectorBox] = await Promise.all([
    page.locator('.demo-detail').boundingBox(),
    inspector.boundingBox(),
  ]);
  expect(detailBox).not.toBeNull();
  expect(inspectorBox).not.toBeNull();
  expect(detailBox!.x + detailBox!.width).toBeLessThanOrEqual(inspectorBox!.x + 1);
  const label = page.getByRole('textbox', { name: 'Label' });
  await label.fill('server');
  await expect(chip).toContainText('server');
  await expect(inspector).toBeVisible();
  await page.locator('wa-select[name="variant"]').evaluate((node: HTMLElement & { value: string }) => {
    node.value = 'success'; node.dispatchEvent(new Event('change', { bubbles: true }));
  });
  await expect(chip).toHaveAttribute('variant', 'success');
  await expect(inspector).toBeVisible();
  await inspector.locator('wa-checkbox[name="pill"]').click();
  await expect(inspector).toBeVisible();
  await chip.locator('[part~="remove-button"]').click();
  await expect(page.getByText('Remove requested for demo-tag')).toBeVisible();
  await expect(inspector).toBeVisible();
  await toggle.click();
  await expect(inspector).toBeHidden();
  await expect(toggle).toContainText('Settings');
  await expect(toggle).toHaveAttribute('aria-expanded', 'false');
});
