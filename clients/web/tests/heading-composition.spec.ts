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
  await demo
    .locator('[data-component="toolbar"]')
    .first()
    .screenshot({ path: 'target/visual-captures/hs2-agdj6e-workspace-heading-wide.png' });

  for (const width of [1440, 390]) {
    await page.setViewportSize({ width, height: 900 });
    await page.goto('/ux-demo?component=connection-details-dialog');
    const dialog = page.getByRole('dialog', { name: 'Server build details' });
    await expect(dialog).toBeVisible();
    await expect(dialog).toHaveAttribute('aria-describedby', 'connection-details-summary');
    const toolbar = dialog.locator('[data-component="toolbar"]');
    await expect(toolbar).not.toHaveAttribute('divider-sides');
    // With a description, Kerf moves the title id onto the title span (KF-DXVMBN, HS2-XHC39W).
    await expect(toolbar.locator('#connection-details-title')).toHaveText('Server build details');
    await expect(toolbar.locator('#connection-details-summary')).toHaveClass('kui-toolbar-text__description');
    await expect(dialog.getByRole('heading', { name: 'Server build details' })).toHaveCount(0);
    await expect(dialog.locator('#connection-details-summary')).not.toHaveText('');
    // Kerf's icon tile: a 16px glyph centered in the group's control slot, like an icon button.
    const iconTile = toolbar.locator('[data-component="toolbar-control-group"]').first();
    await expect(iconTile).toHaveAttribute('data-tile-tone', 'brand');
    await expect(iconTile.locator(':scope > svg')).toHaveCSS('width', '16px');
    await expect(iconTile.locator(':scope > svg')).toHaveCSS('height', '16px');
    // Kerf stacks the supporting line under the title text, starting where the title starts.
    const lines = await toolbar.evaluate((node) => {
      const title = node.querySelector('#connection-details-title')!.getBoundingClientRect();
      const summary = node.querySelector('#connection-details-summary')!.getBoundingClientRect();
      const icon = node.querySelector('[data-component="toolbar-control-group"]')!.getBoundingClientRect();
      return {
        startDelta: Math.abs(title.left - summary.left),
        below: summary.top >= title.bottom - 1,
        afterIcon: title.left > icon.right,
      };
    });
    expect(lines.startDelta).toBeLessThan(1);
    expect(lines.below).toBe(true);
    expect(lines.afterIcon).toBe(true);
    await dialog.screenshot({ path: `target/visual-captures/hs2-agdj6e-dialog-heading-${width}.png` });
  }
});
