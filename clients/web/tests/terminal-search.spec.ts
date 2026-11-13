import { expect, type Page, test } from '@playwright/test';

import { installTerminalFixture } from './terminal-feedback-fixture';

// HS2-TWZSZF: a borderless, iconic search button next to the drawer's hide button expands into a field
// when activated and searches the selected terminal.
test.use({ viewport: { width: 1280, height: 800 } });

async function openDrawerTerminal(page: Page) {
  await installTerminalFixture(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open Project' }).click();
  await page.getByRole('button', { name: 'Local Project' }).click();
  await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();
  await page.getByRole('button', { name: 'Show Terminal Drawer' }).click();
  const drawer = page.locator('[data-component="terminal-drawer"]');
  await drawer.getByRole('button', { name: 'New Drawer Item' }).click();
  await drawer.getByRole('menuitem', { name: 'Terminal' }).click();
  const viewport = drawer.locator('[data-component="terminal-session"] [data-terminal-id="terminal-new"]');
  await expect(viewport).toHaveAttribute('data-connection', 'connected');
  await expect(viewport.locator('.xterm-rows')).toContainText('GNU nano 8.4');
  return { drawer, viewport };
}

test('HS2-TWZSZF terminal search expands from a borderless icon, selects matches, and collapses again', async ({
  page,
}) => {
  const { drawer, viewport } = await openDrawerTerminal(page);
  const group = drawer.locator('.kui-toolbar-control-group[data-content="search"]'),
    trigger = group.getByRole('button', { name: 'Search Terminal' }),
    editor = group.locator('[data-search-field-editor="terminal-search"]'),
    // The active match is the terminal's selection, read through the viewport's clipboard event.
    selected = () =>
      viewport.evaluate((node) => {
        const detail: { selectionOnly: boolean; text?: string } = { selectionOnly: true };
        node.dispatchEvent(new CustomEvent('hotsheet-terminal-read-text', { detail }));
        return detail.text ?? '';
      });
  // Collapsed: one iconic, borderless control immediately before the Clear and Hide actions.
  await expect(group).toHaveAttribute('data-appearance', 'borderless');
  await expect(group).toHaveAttribute('data-expanded', 'false');
  await expect(trigger.locator('[data-lucide="search"]')).toBeVisible();
  // Borderless until activated: no visible border or fill around the icon.
  await expect(group).toHaveCSS('border-top-color', 'rgba(0, 0, 0, 0)');
  await expect(group).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
  const triggerBox = (await trigger.boundingBox())!,
    clearBox = (await drawer.getByRole('button', { name: 'Clear Terminal' }).boundingBox())!;
  expect(triggerBox.x + triggerBox.width).toBeLessThanOrEqual(clearBox.x);
  await page.screenshot({ path: test.info().outputPath('hs2-twzszf-collapsed-1280.png') });

  // Activated: Kerf expands the field and focuses its editor.
  await trigger.click();
  await expect(group).toHaveAttribute('data-expanded', 'true');
  await expect(group).toHaveAttribute('data-appearance', 'contained');
  await expect(editor).toBeFocused();

  // Typing searches incrementally and selects the match; Enter and Shift-Enter step without leaving the field.
  await page.keyboard.type('Justify');
  await expect.poll(selected).toBe('Justify');
  await page.keyboard.press('Enter');
  await expect.poll(selected).toBe('Justify');
  await page.keyboard.press('Shift+Enter');
  await expect(editor).toBeFocused();
  await expect(editor).toHaveText('Justify');
  await expect.poll(selected).toBe('Justify');
  await page.screenshot({ path: test.info().outputPath('hs2-twzszf-match-1280.png') });

  // A query with no match selects nothing; a new query finds its match; emptying the field clears it.
  await page.keyboard.type('-absent');
  await expect.poll(selected).toBe('');
  await page.keyboard.press('ControlOrMeta+a');
  await page.keyboard.press('Backspace');
  await page.keyboard.type('nano');
  await expect.poll(selected).toBe('nano');
  await page.keyboard.press('ControlOrMeta+a');
  await page.keyboard.press('Backspace');
  await expect.poll(selected).toBe('');

  // Escape on the empty field collapses it back to the iconic button and returns focus there.
  await page.keyboard.press('Escape');
  await expect(group).toHaveAttribute('data-expanded', 'false');
  await expect(group).toHaveAttribute('data-appearance', 'borderless');
  await expect(trigger).toBeFocused();

  // The grid view has no single terminal to search.
  await drawer.getByRole('button', { name: 'Project Grid' }).click();
  await expect(drawer.locator('.kui-toolbar-control-group[data-content="search"]')).toHaveCount(0);
});
