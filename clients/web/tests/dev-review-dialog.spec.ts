import { expect, test } from '@playwright/test';

// The Dev Review overlay copies the @kerfjs/ui DialogHeader markup (it is framework-neutral and
// cannot import kerf). This pins that copy to the new Toolbar-based DialogHeader anatomy so the
// shared kui-dialog-header CSS styles it correctly (HS2-M4X0WS).
test('dev-review new-ticket dialog uses the current DialogHeader anatomy', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto('/');
  const tool = page.locator('.hs-dev-review');
  await expect(tool.getByRole('button', { name: 'Feedback' })).toBeVisible();
  await tool.getByRole('button', { name: 'Feedback' }).click();
  await tool.getByRole('button', { name: 'New Ticket' }).click();
  const header = page.locator('.hs-dev-review__form .kui-dialog-header');
  await expect(header).toBeVisible();
  await expect(header).toHaveAttribute('data-has-summary', 'true');
  // A real top Toolbar carries the identity (icon+title) and the grouped, labeled actions.
  await expect(header.locator('.kui-dialog-header__toolbar.kui-toolbar')).toBeVisible();
  await expect(header.locator('.kui-toolbar__leading .kui-dialog-header__identity .kui-dialog-header__copy h2')).toHaveText('New Hot Sheet ticket');
  const actions = header.locator('.kui-toolbar__trailing .kui-dialog-header__actions');
  await expect(actions).toHaveAttribute('aria-label', 'New ticket actions');
  await expect(actions.locator('.hs-dev-review__close')).toBeVisible();
  // The summary is a sibling below the toolbar, not nested inside the title copy.
  await expect(header.locator('.kui-dialog-header__summary')).toHaveText('Attach visual context and describe the change you need.');
  expect(await header.locator('.kui-dialog-header__copy .kui-dialog-header__summary').count()).toBe(0);
  await header.screenshot({ path: '/private/tmp/hs2-m4x0ws-devreview-header-wide.png' });
});
