import { expect, test } from '@playwright/test';

test('catalog halt prompt wires all actions and fits a phone viewport (HS2-E6KAWY)', async ({ page }) => {
  await page.goto('/ux-demo?component=halted-session-popup&dev-review=false');
  const popup = page.getByRole('dialog', { name: 'AI session halted' });
  await expect(popup.locator('.halted-session-popup__identity')).toContainText('Claude main worker');
  await expect(popup.locator('.halted-session-popup__project')).toContainText('Hot Sheet demo');
  await expect(popup.locator('.halted-session-popup__details')).toContainText('Selected model is at capacity');
  await page.screenshot({ path: test.info().outputPath('halted-session-card-wide.png') });
  for (const action of ['Open session', 'Dismiss', 'Pause notifications']) {
    await expect(popup).toBeVisible();
    await popup.getByRole('button', { name: action, exact: true }).click();
    await expect(popup).toHaveCount(0);
    await page.getByRole('button', { name: 'Show halted session', exact: true }).click();
  }
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(popup).toBeInViewport({ ratio: 1 });
  await expect(popup.getByRole('button', { name: 'Pause notifications', exact: true })).toBeInViewport({ ratio: 1 });
  await page.screenshot({ path: test.info().outputPath('halted-session-card-phone.png') });
});
