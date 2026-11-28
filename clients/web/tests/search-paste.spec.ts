import { expect, type Locator, type Page, test } from '@playwright/test';

import { openDemoProject } from './mobile-project-fixture';
import { openMockedProject } from './open-mocked-project';
import { installTerminalFixture } from './terminal-feedback-fixture';

// HS2-PQWZRG: a paste copied from a web page into any search field inserts its plain text only, on
// one line, at the field's own type size, with the caret after it, and Undo removes it. Every
// search field is Kerf's contenteditable SearchField; the app's other filter inputs are native
// inputs that the browser already keeps plain.

test.use({ viewport: { width: 1280, height: 800 } });

const html = '<h1 style="font-size: 48px"><b>Big</b> title</h1><p style="color: red">line two</p>';
const plain = 'Big title\nline two';
const pasted = 'Big title line two';

/**
 * Paste a rich clipboard into the focused editor: the paste event the browser sends for a web
 * page copy, carrying `text/html` and `text/plain`. The operating system clipboard is shared by
 * parallel workers, and WebKit refuses scripted clipboard writes, so neither engine uses it. A
 * dispatched paste has no default action: only the fields' own paste handling can insert text.
 */
async function pasteRich(editor: Locator, text = plain) {
  await editor.evaluate(
    (node, { html, text }) => {
      const clipboardData = new DataTransfer();
      clipboardData.setData('text/html', html);
      clipboardData.setData('text/plain', text);
      node.dispatchEvent(new ClipboardEvent('paste', { bubbles: true, cancelable: true, clipboardData }));
    },
    { html, text },
  );
}

/** The pasted editor holds only text, every descendant at the editor's own type size. */
async function expectPlainEditor(editor: Locator, text: string) {
  await expect(editor).toHaveText(text);
  await expect(editor.locator('h1, b, p, div, br, [style]')).toHaveCount(0);
  const sizes = await editor.evaluate((node) => ({
    editor: getComputedStyle(node).fontSize,
    descendants: [...node.querySelectorAll('*')].map((child) => getComputedStyle(child).fontSize),
  }));
  for (const size of sizes.descendants) expect(size).toBe(sizes.editor);
  expect(Number.parseFloat(sizes.editor)).toBeLessThan(20);
}

/**
 * Type around a paste in the middle of the query: the paste lands at the caret, on one line, the
 * caret ends after it, the field's own value follows, Undo removes the paste, and a second paste
 * and typing still work.
 */
async function exercisePaste(page: Page, browserName: string, editor: Locator, value?: Locator) {
  await page.keyboard.type('ab');
  await page.keyboard.press('ArrowLeft');
  await pasteRich(editor);
  await expectPlainEditor(editor, `a${pasted}b`);
  await page.keyboard.type('Z');
  await expectPlainEditor(editor, `a${pasted}Zb`);
  // Kerf's model received the pasted text as an ordinary edit.
  if (value) await expect(value).toHaveValue(`a${pasted}Zb`);
  await page.keyboard.press('ControlOrMeta+z');
  if (browserName === 'webkit') {
    // Playwright's WebKit undoes a field's whole edit history on one press, even in a native
    // input, so assert only that Undo reaches back past the paste.
    await expect(page.locator('[data-search-field-editor]').filter({ hasText: 'Big title' })).toHaveCount(0);
    return;
  }
  // Chromium steps back one edit at a time: the typed Z, then the whole paste.
  await expectPlainEditor(editor, `a${pasted}b`);
  await page.keyboard.press('ControlOrMeta+z');
  await expectPlainEditor(editor, 'ab');
  await pasteRich(editor, 'again');
  await page.keyboard.type('!');
  await expectPlainEditor(editor, 'aagain!b');
  // A paste straight after typing is still its own Undo step, apart from the keys before it.
  await pasteRich(editor, 'X');
  await expectPlainEditor(editor, 'aagain!Xb');
  await page.keyboard.press('ControlOrMeta+z');
  await expectPlainEditor(editor, 'aagain!b');
  if (value) await expect(value).toHaveValue('aagain!b');
}

test('HS2-PQWZRG workspace search pastes plain text with the caret after it and undoes the paste', async ({
  page,
  browserName,
}) => {
  await openDemoProject(page);
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.getByRole('button', { name: 'Search tickets' }).click();
  const search = page.getByRole('searchbox', { name: 'Search tickets' });
  await expect(search).toBeFocused();
  await exercisePaste(page, browserName, search);
  await page.screenshot({ path: test.info().outputPath(`hs2-pqwzrg-workspace-${browserName}-1280.png`) });
});

test('HS2-PQWZRG the saved view query pastes plain text that reaches its query value', async ({
  page,
  browserName,
}) => {
  await openDemoProject(page);
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.getByRole('button', { name: 'Add view' }).click();
  const dialog = page.locator('[data-dialog-component="saved-view-dialog"]');
  const query = dialog.getByRole('searchbox', { name: 'Search query' });
  await query.click();
  await expect(query).toBeFocused();
  await exercisePaste(page, browserName, query, dialog.locator('input[name="saved-view-query"]'));
});

test('HS2-PQWZRG guidance search pastes plain text', async ({ page, browserName }) => {
  await openDemoProject(page);
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.getByLabel('Settings View').click();
  await page.getByRole('button', { name: 'Project guidance' }).click();
  await page.getByRole('button', { name: 'Search guidance rules' }).click();
  const search = page.getByRole('searchbox', { name: 'Search guidance rules' });
  await expect(search).toBeFocused();
  await exercisePaste(page, browserName, search);
});

test('HS2-PQWZRG terminal search pastes plain text and searches for it', async ({ page, browserName }) => {
  await installTerminalFixture(page);
  await page.goto('/');
  await openMockedProject(page);
  await page.getByRole('button', { name: 'Show Terminal Drawer' }).click();
  await expect(page.locator('.app-shell')).toHaveAttribute('data-terminal-drawer-transitioning', 'false');
  const drawer = page.locator('[data-component="terminal-drawer"]');
  await drawer.getByRole('button', { name: 'New Drawer Item' }).click();
  await drawer.getByRole('menuitem', { name: 'Terminal' }).click();
  const viewport = drawer.locator('[data-component="terminal-session"] [data-terminal-id="terminal-new"]');
  await expect(viewport.locator('.xterm-rows')).toContainText('GNU nano 8.4');
  const group = drawer.locator('.kui-toolbar-control-group[data-content="search"]'),
    editor = group.locator('[data-search-field-editor="terminal-search"]');
  await group.getByRole('button', { name: 'Search Terminal' }).click();
  await expect(editor).toBeFocused();
  // A pasted query searches the terminal like a typed one.
  await pasteRich(editor, 'Justify');
  await expectPlainEditor(editor, 'Justify');
  await expect
    .poll(() =>
      viewport.evaluate((node) => {
        const detail: { selectionOnly: boolean; text?: string } = { selectionOnly: true };
        node.dispatchEvent(new CustomEvent('hotsheet-terminal-read-text', { detail }));
        return detail.text ?? '';
      }),
    )
    .toBe('Justify');
  await page.keyboard.press('ControlOrMeta+a');
  await page.keyboard.press('Backspace');
  await expectPlainEditor(editor, '');
  await exercisePaste(page, browserName, editor);
});
