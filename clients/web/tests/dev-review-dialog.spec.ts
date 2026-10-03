import { expect, test } from '@playwright/test';

// The framework-neutral overlay uses the canonical Toolbar/ToolbarText anatomy with app-owned supporting copy.
test('dev-review new-ticket dialog uses the canonical Toolbar heading anatomy', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto('/?dev-review=1');
  const tool = page.locator('.hs-dev-review');
  await expect(tool.getByRole('button', { name: 'Feedback' })).toBeVisible();
  await tool.getByRole('button', { name: 'Feedback' }).click();
  await tool.getByRole('button', { name: 'New Ticket' }).click();
  const header = page.locator('.hs-dev-review__form .app-heading');
  await expect(header).toBeVisible();
  await expect(header).toHaveAttribute('data-has-summary', 'true');
  // A real top Toolbar carries the extra-large title and the grouped, labeled actions.
  await expect(header.locator('.kui-toolbar')).toBeVisible();
  await expect(header.locator('.kui-toolbar__leading .kui-toolbar-text')).toHaveText('New Hot Sheet ticket');
  const actions = header.locator('.kui-toolbar__trailing .kui-toolbar-control-group');
  await expect(actions).toHaveAttribute('aria-label', 'New ticket actions');
  await expect(actions.locator('.hs-dev-review__close')).toBeVisible();
  // The summary is a sibling below the toolbar, not nested inside the title.
  await expect(header.locator('.app-heading__summary')).toHaveText(
    'Attach visual context and describe the change you need.',
  );
  expect(await header.locator('.kui-toolbar .app-heading__summary').count()).toBe(0);
  // The heading's small-summary variant and the form's reserved first row replace the former
  // `.hs-dev-review__dialog .app-heading` restyle (HS2-R9GQJE).
  await expect(header).toHaveAttribute('data-summary-size', 'small');
  const sizes = await header.evaluate((element) => {
    const probe = document.createElement('span');
    probe.style.fontSize = 'var(--wa-font-size-xs)';
    element.append(probe);
    const result = {
      summary: getComputedStyle(element.querySelector('.app-heading__summary')!).fontSize,
      xs: getComputedStyle(probe).fontSize,
    };
    probe.remove();
    return result;
  });
  expect(sizes.summary).toBe(sizes.xs);
  expect((await header.boundingBox())!.height).toBeGreaterThanOrEqual(78);
  await header.screenshot({ path: test.info().outputPath('devreview-heading-wide.png') });
  await page.setViewportSize({ width: 390, height: 900 });
  await expect(header.getByRole('button', { name: 'Close new ticket dialog' })).toBeInViewport();
  await header.screenshot({ path: test.info().outputPath('devreview-heading-narrow.png') });
  await page.emulateMedia({ colorScheme: 'dark' });
  await expect(page.locator('.hs-dev-review__dialog')).toHaveCSS('background-color', 'rgb(28, 28, 30)');
  await header.screenshot({ path: test.info().outputPath('devreview-heading-dark.png') });
  await header.getByRole('button', { name: 'Close new ticket dialog' }).click();
  await expect(header).toBeHidden();
});
