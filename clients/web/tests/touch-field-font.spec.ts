import { expect, type Page, test } from '@playwright/test';

// HS2-8JPRRC: iOS zooms the page into any focused text field set below 16px, so touch screens keep
// every native text field at least that size while desktop keeps the compact type.

/** Computed font size (px) of every element matching `selector`, reading a custom element's inner field. */
const fontSizes = (page: Page, selector: string) =>
  page.evaluate(
    (sel) =>
      [...document.querySelectorAll(sel)].map((element) => {
        const field = (element as HTMLElement).shadowRoot?.querySelector('input, textarea') ?? element;
        return parseFloat(getComputedStyle(field).fontSize);
      }),
    selector,
  );

/** Open every field state the composer, inspector, attachments, and provider form demos expose. */
async function collect(page: Page) {
  const sizes: Record<string, number[]> = {};
  await page.goto('/ux-demo?component=quick-ticket-composer');
  await page.locator('[data-component="quick-ticket-composer-launcher"]').first().click();
  await expect(page.locator('[name="new-ticket-details"]')).toBeVisible();
  sizes.details = await fontSizes(page, '[name="new-ticket-details"]');
  await page.goto('/ux-demo?component=ticket-inspector');
  const inspector = page.locator('[data-component="ticket-inspector"]');
  await inspector.getByRole('button', { name: 'Edit Ticket details' }).getByRole('heading').first().click();
  await expect(page.locator('[name="markdown-source"]')).toBeVisible();
  sizes.markdown = await fontSizes(page, '[name="markdown-source"]');
  await page.goto('/ux-demo?component=ticket-inspector');
  await page.locator('[data-component="ticket-inspector"]').getByRole('button', { name: 'Add tag' }).click();
  await expect(page.locator('[name="ticket-tag-input"]')).toBeVisible();
  sizes.tag = await fontSizes(page, '[name="ticket-tag-input"]');
  await page.goto('/ux-demo?component=ticket-attachments');
  await expect(page.locator('[name="attachment-batch-purpose"]').first()).toBeVisible();
  sizes.purpose = await fontSizes(page, '[name="attachment-batch-purpose"]');
  await page.goto('/ux-demo?component=provider-setup-form');
  await expect(page.locator('input.provider-setup-form__field-control[name="connection-locator"]')).toBeVisible();
  sizes.locator = await fontSizes(page, 'input.provider-setup-form__field-control');
  return sizes;
}

test.describe('touch phone', () => {
  test.use({ viewport: { width: 390, height: 844 }, hasTouch: true, isMobile: true });
  test('keeps native text fields at 16px or larger so iOS never zooms on focus', async ({ page }) => {
    const sizes = await collect(page);
    for (const [field, values] of Object.entries(sizes)) {
      expect(values.length, field).toBeGreaterThan(0);
      for (const value of values) expect(value, field).toBeGreaterThanOrEqual(16);
    }
    // The purpose pill grows with its text instead of clipping it.
    const pill = page.locator('[name="attachment-batch-purpose"]').first();
    await page.goto('/ux-demo?component=ticket-attachments');
    const box = (await pill.boundingBox())!;
    expect(box.height).toBeGreaterThanOrEqual(
      await pill.evaluate((node) => parseFloat(getComputedStyle(node).lineHeight) || 16),
    );
    await page.screenshot({ path: test.info().outputPath('hs2-8jprrc-attachments-390.png') });
  });
});

test.describe('desktop', () => {
  test.use({ viewport: { width: 1280, height: 900 }, hasTouch: false, isMobile: false });
  test('keeps the compact field type with a mouse', async ({ page }) => {
    const sizes = await collect(page);
    expect(sizes.details).toEqual([14]);
    expect(sizes.markdown).toEqual([14]);
    expect(sizes.tag).toEqual([12]);
    expect(new Set(sizes.purpose)).toEqual(new Set([12]));
    expect(new Set(sizes.locator)).toEqual(new Set([12]));
  });
});
