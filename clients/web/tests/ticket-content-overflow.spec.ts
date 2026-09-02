import { expect, test } from '@playwright/test';

test('keeps details, metadata, and notes inside the ticket inspector at narrow and wide viewports', async ({ page }) => {
  await page.setViewportSize({ width: 820, height: 900 });
  await page.goto('/ux-demo?component=ticket-inspector');

  const inspector = page.locator('[data-component="ticket-inspector"]');
  const content = inspector.locator('.ticket-inspector__content');
  await expect(inspector).toBeVisible();
  const containment = await content.evaluate(node => {
    const details = node.querySelector('.markdown-preview');
    const note = node.querySelector('.note-card__body');
    if (!details || !note) throw new Error('Expected details and note content');
    details.textContent = 'details/'.repeat(180);
    note.textContent = 'note/'.repeat(180);
    const contentRect = node.getBoundingClientRect();
    const surfaces = [
      ...node.querySelectorAll('.ticket-inspector__metadata > .select, .ticket-inspector__details-surface, .note-card'),
    ];
    return {
      clientWidth: node.clientWidth,
      scrollWidth: node.scrollWidth,
      children: surfaces.map(surface => {
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

  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto('/ux-demo?component=ticket-inspector');
  const wideContent = page.locator('[data-component="ticket-inspector"] .ticket-inspector__content');
  await expect.poll(() => wideContent.evaluate(node => node.scrollWidth <= node.clientWidth)).toBe(true);
});
