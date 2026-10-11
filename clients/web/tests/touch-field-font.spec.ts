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

/**
 * Every editable native text field in the page, including inside shadow roots, whose computed font
 * size is under 16px. Read-only fields never raise the keyboard, and Web Awesome's hidden form-value
 * mirrors (`aria-hidden`, `tabindex=-1`) never take focus, so neither can trigger iOS focus zoom.
 */
const smallEditableFields = (page: Page) =>
  page.evaluate(() => {
    const found: string[] = [];
    const nonText = new Set([
      'checkbox',
      'radio',
      'range',
      'color',
      'file',
      'button',
      'submit',
      'reset',
      'hidden',
      'image',
    ]);
    const walk = (root: Document | ShadowRoot, host: string) => {
      for (const element of root.querySelectorAll<HTMLElement>('*')) {
        if (element.shadowRoot) walk(element.shadowRoot, element.localName);
        const field = element as HTMLInputElement;
        const editable =
          element.isContentEditable ||
          element.localName === 'textarea' ||
          element.localName === 'select' ||
          (element.localName === 'input' && !nonText.has(field.type));
        if (!editable || field.readOnly || field.disabled) continue;
        if (element.getAttribute('aria-hidden') === 'true' && element.tabIndex < 0) continue;
        // xterm's input proxy is offscreen and sized by the terminal, not a typed text field.
        if (element.classList.contains('xterm-helper-textarea')) continue;
        const size = parseFloat(getComputedStyle(element).fontSize);
        if (size < 16) found.push(`${host} ${element.localName}[name=${element.getAttribute('name') ?? ''}] ${size}px`);
      }
    };
    walk(document, 'document');
    return found;
  });

test.describe('touch phone audit (HS2-JDXSND)', () => {
  test.use({ viewport: { width: 390, height: 844 }, hasTouch: true, isMobile: true });
  test('every UX demo keeps its editable text fields at 16px or larger', async ({ page }) => {
    test.setTimeout(300_000);
    const small: string[] = [];
    await page.goto('/ux-demo');
    // The Vite dev server serves the catalog module, so the walk always covers every listed demo.
    const ids = await page.evaluate(async (module) => {
      const catalog = (await import(module)) as { flattenCatalog: () => Array<{ id: string }> };
      return catalog.flattenCatalog().map((demo) => demo.id);
    }, '/src/ux-demo/catalog.ts');
    expect(ids.length).toBeGreaterThan(50);
    for (const id of ids) {
      await page.goto(`/ux-demo?component=${id}`);
      await expect(page.locator('#ux-demo-stage, main').first()).toBeAttached();
      small.push(...(await smallEditableFields(page)).map((field) => `${id}: ${field}`));
    }
    expect(small).toEqual([]);
  });

  test('the Not working note stays at 16px when focused', async ({ page }) => {
    await page.goto('/ux-demo?component=not-working-dialog');
    const note = page.locator('textarea[name="not-working-note"]').first();
    await page.locator('[data-action="open-not-working-demo"]').tap();
    await expect(note).toBeVisible();
    await note.tap();
    expect(await note.evaluate((element) => parseFloat(getComputedStyle(element).fontSize))).toBeGreaterThanOrEqual(16);
    await page.screenshot({ path: test.info().outputPath('hs2-jdxsnd-not-working-390.png') });
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
  test('keeps the compact Not working note with a mouse (HS2-JDXSND)', async ({ page }) => {
    await page.goto('/ux-demo?component=not-working-dialog');
    await page.locator('[data-action="open-not-working-demo"]').click();
    const note = page.locator('textarea[name="not-working-note"]').first();
    await expect(note).toBeVisible();
    expect(await note.evaluate((element) => parseFloat(getComputedStyle(element).fontSize))).toBe(14);
    await page.screenshot({ path: test.info().outputPath('hs2-jdxsnd-not-working-1280.png') });
  });
});
