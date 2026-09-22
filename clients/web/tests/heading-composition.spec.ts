import { expect, test } from '@playwright/test';

// Page and dialog titles share primitives, but retain distinct accessibility semantics.
test('preserves page headings and dialog naming with direct Toolbar compositions (HS2-AGDJ6E)', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto('/ux-demo?component=workspace-header');
  const demo = page.locator('[aria-label="WorkspaceHeader demo"]');
  await expect(demo.getByRole('heading', { name: 'Queue', level: 1 })).toBeVisible();
  await demo.getByRole('button', { name: 'Notifications view' }).click();
  await expect(demo.getByRole('heading', { name: 'Notifications', level: 1 })).toBeVisible();
  await demo.getByRole('button', { name: 'List view' }).click();
  await expect(demo.getByRole('heading', { name: 'Queue', level: 1 })).toBeVisible();
  await demo.locator('.app-heading').screenshot({ path: '/private/tmp/hs2-agdj6e-workspace-heading-wide.png' });

  for (const width of [1440, 390]) {
    await page.setViewportSize({ width, height: 900 });
    await page.goto('/ux-demo?component=connection-details-dialog');
    // The catalog has a desktop-only shell; inspect its production dialog at the actual viewport width.
    if (width < 600)
      await page.addStyleTag({
        content:
          'body{min-width:0}.demo-shell{display:block}.kui-catalog__sidebar,.kui-catalog__header,.kui-catalog__footer,.settings-toggle{display:none}.kui-catalog__detail{min-height:0;padding:12px}',
      });
    const dialog = page.getByRole('dialog', { name: 'Server build details' });
    await expect(dialog).toBeVisible();
    await expect(dialog).toHaveAttribute('aria-describedby', 'connection-details-summary');
    const toolbar = dialog.locator('[data-component="toolbar"]');
    await expect(toolbar).not.toHaveAttribute('divider-sides');
    await expect(toolbar.locator('[data-component="toolbar-text"]')).toHaveAttribute('id', 'connection-details-title');
    await expect(dialog.getByRole('heading', { name: 'Server build details' })).toHaveCount(0);
    await expect(dialog.locator('#connection-details-summary')).not.toHaveText('');
    await expect(toolbar.locator('.app-heading__symbol')).toHaveCSS('width', '22px');
    await expect(toolbar.locator('.app-heading__symbol')).toHaveCSS('height', '22px');
    expect(
      await toolbar.evaluate((node) => {
        const title = node.querySelector('[data-component="toolbar-text"]')!.getBoundingClientRect();
        const icon = node.querySelector('[data-component="toolbar-control-group"]')!.getBoundingClientRect();
        return Math.abs(title.y + title.height / 2 - icon.y - icon.height / 2);
      }),
    ).toBeLessThan(1);
    await dialog.screenshot({ path: `/private/tmp/hs2-agdj6e-dialog-heading-${width}.png` });
  }
});
