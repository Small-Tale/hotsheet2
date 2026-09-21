import { expect, test } from '@playwright/test';

// The Dev Review overlay copies the @kerfjs/ui PanelHeader markup (it is framework-neutral and
// cannot import kerf). This pins that copy to the current Toolbar-based PanelHeader anatomy so the
// shared kui-panel-header CSS styles it correctly (HS2-M4X0WS).
test('dev-review new-ticket dialog uses the current PanelHeader anatomy', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto('/');
  const tool = page.locator('.hs-dev-review');
  await expect(tool.getByRole('button', { name: 'Feedback' })).toBeVisible();
  await tool.getByRole('button', { name: 'Feedback' }).click();
  await tool.getByRole('button', { name: 'New Ticket' }).click();
  const header = page.locator('.hs-dev-review__form .kui-panel-header');
  await expect(header).toBeVisible();
  await expect(header).toHaveAttribute('data-has-summary', 'true');
  // A real top Toolbar carries the extra-large title and the grouped, labeled actions.
  await expect(header.locator('.kui-toolbar')).toBeVisible();
  await expect(header.locator('.kui-toolbar__leading .kui-panel-header__title')).toHaveText('New Hot Sheet ticket');
  const actions = header.locator('.kui-toolbar__trailing .kui-toolbar-control-group');
  await expect(actions).toHaveAttribute('aria-label', 'New ticket actions');
  await expect(actions.locator('.hs-dev-review__close')).toBeVisible();
  // The summary is a sibling below the toolbar, not nested inside the title.
  await expect(header.locator('.kui-panel-header__summary')).toHaveText(
    'Attach visual context and describe the change you need.',
  );
  expect(await header.locator('.kui-toolbar .kui-panel-header__summary').count()).toBe(0);
  await header.screenshot({ path: '/private/tmp/hs2-m4x0ws-devreview-header-wide.png' });
});
