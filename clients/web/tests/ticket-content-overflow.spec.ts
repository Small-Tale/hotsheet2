import { expect, test } from '@playwright/test';

test('keeps details, metadata, and notes inside the ticket inspector at narrow and wide viewports', async ({
  page,
}) => {
  await page.setViewportSize({ width: 820, height: 900 });
  await page.goto('/ux-demo?component=ticket-inspector');

  const inspector = page.locator('[data-component="ticket-inspector"]');
  const content = inspector.locator('.ticket-inspector-panel');
  await expect(inspector).toBeVisible();
  // Enter the unbroken strings through the real editors so the demo's state owns them. Writing
  // `textContent` directly was undone by any later re-render, which full-suite load made likely
  // (HS2-Y8VMXD).
  await inspector.getByRole('button', { name: 'Edit Ticket details' }).getByRole('heading').first().click();
  const detailsEditor = inspector.getByRole('textbox', { name: 'Ticket details' });
  await detailsEditor.fill('details/'.repeat(180));
  await detailsEditor.blur();
  await expect(detailsEditor).toHaveCount(0);
  const note = content.locator('[data-component="note-card"]').first();
  await note.locator('.note-card__body').click({ position: { x: 4, y: 4 } });
  const noteEditor = note.getByRole('textbox', { name: 'Note body' });
  await noteEditor.fill('note/'.repeat(180));
  await noteEditor.blur();
  await expect(noteEditor).toHaveCount(0);
  // A deterministic re-render (away to Timeline and back) must keep both: the content is state.
  await inspector.getByRole('tab', { name: 'Timeline' }).click();
  await inspector.getByRole('tab', { name: 'Info' }).click();

  for (const width of [820, 1440, 820]) {
    await page.setViewportSize({ width, height: 900 });
    await expect(content.locator('.markdown-preview').first()).toHaveText('details/'.repeat(180));
    await expect(content.locator('.note-card__body').first()).toHaveText('note/'.repeat(180));
    const containment = await content.evaluate((node) => {
      const contentRect = node.getBoundingClientRect();
      const surfaces = [
        ...node.querySelectorAll(
          '.ticket-info-panel__metadata > .kui-select, .ticket-info-panel__details-surface, .note-card',
        ),
      ];
      return {
        clientWidth: node.clientWidth,
        scrollWidth: node.scrollWidth,
        children: surfaces.map((surface) => {
          const rect = surface.getBoundingClientRect();
          return { left: rect.left, right: rect.right };
        }),
        left: contentRect.left,
        right: contentRect.right,
      };
    });

    expect(containment.scrollWidth).toBeLessThanOrEqual(containment.clientWidth);
    expect(containment.children.length).toBeGreaterThanOrEqual(4);
    for (const child of containment.children) {
      expect(child.left).toBeGreaterThanOrEqual(containment.left - 1);
      expect(child.right).toBeLessThanOrEqual(containment.right + 1);
    }
    await expect(content.locator('.note-card__body').first()).toHaveCSS('overflow-wrap', 'anywhere');
  }
});
