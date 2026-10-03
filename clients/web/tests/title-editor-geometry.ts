import { expect, type Locator, type Page } from '@playwright/test';

/** Rendered text lines in a title heading or the wrapping title editor (HS2-98ZVPE). */
export function renderedTitleLines(locator: Locator): Promise<number> {
  return locator.evaluate((node) => {
    const style = getComputedStyle(node),
      content = node.clientHeight - parseFloat(style.paddingTop) - parseFloat(style.paddingBottom);
    return Math.round(content / parseFloat(style.lineHeight));
  });
}

/**
 * Edits a ticket title through its wrapping editor and asserts the HS2-98ZVPE contract: the editor shows
 * every wrapped line the static heading shows, never clips the title horizontally or vertically, grows
 * as text is added, collapses inserted line breaks, and finishes on Enter without inserting a newline.
 * Returns the saved single-line title.
 */
export async function editLongTitleThroughWrappingEditor(
  page: Page,
  surface: Locator,
  openEditor: () => Promise<void>,
  longTitle: string,
  editingScreenshot?: string,
): Promise<string> {
  const editor = surface.getByRole('textbox', { name: 'Ticket title' }),
    width = page.viewportSize()!.width;
  await openEditor();
  await expect(editor).toBeFocused();
  await editor.fill(longTitle);
  await editor.press('Enter');
  await expect(editor).toHaveCount(0);
  const heading = surface.getByRole('heading', { name: longTitle });
  await expect(heading).toBeVisible();
  const headingLines = await renderedTitleLines(heading);

  await openEditor();
  await expect(editor).toBeFocused();
  await expect(editor).toHaveJSProperty('value', longTitle);
  expect(await editor.evaluate((node) => node.tagName)).toBe('TEXTAREA');
  // Every line the static title wraps onto is visible while editing, with nothing scrolled out of view.
  expect(await renderedTitleLines(editor)).toBe(headingLines);
  const fits = () =>
    editor.evaluate((node) => ({
      horizontal: node.scrollWidth <= node.clientWidth,
      vertical: node.scrollHeight <= node.clientHeight + 1,
    }));
  expect(await fits()).toEqual({ horizontal: true, vertical: true });
  const box = (await editor.boundingBox())!;
  expect(box.x).toBeGreaterThanOrEqual(0);
  expect(box.x + box.width).toBeLessThanOrEqual(width);

  // Pasted line breaks collapse to spaces, and the editor grows with the added text.
  await editor.evaluate((node: HTMLTextAreaElement) => {
    node.setSelectionRange(node.value.length, node.value.length);
  });
  await page.keyboard.insertText(
    ' with a pasted\nsecond line\r\n\nand a third line that keeps going until the wrapped editor must grow taller than the title was',
  );
  const saved = `${longTitle} with a pasted second line and a third line that keeps going until the wrapped editor must grow taller than the title was`;
  await expect(editor).toHaveJSProperty('value', saved);
  expect(await renderedTitleLines(editor)).toBeGreaterThan(headingLines);
  expect(await fits()).toEqual({ horizontal: true, vertical: true });
  if (editingScreenshot) await page.screenshot({ path: editingScreenshot });

  // Enter finishes the edit instead of inserting a newline.
  await editor.press('Enter');
  await expect(editor).toHaveCount(0);
  await expect(surface.getByRole('heading', { name: saved })).toBeVisible();
  return saved;
}
