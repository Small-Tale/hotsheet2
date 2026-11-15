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
  await expect(page.locator('.app-shell')).toHaveAttribute('data-terminal-drawer-transitioning', 'false');
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

test('HS2-SW2YAZ double-clicking in the terminal search field never maximizes the drawer', async ({ page }) => {
  const { drawer } = await openDrawerTerminal(page);
  const group = drawer.locator('.kui-toolbar-control-group[data-content="search"]'),
    trigger = group.getByRole('button', { name: 'Search Terminal' }),
    editor = group.locator('[data-search-field-editor="terminal-search"]');
  await expect(drawer).toHaveAttribute('data-maximized', 'false');
  await trigger.click();
  await expect(group).toHaveAttribute('data-expanded', 'true');
  // The reported gesture: double-click in the field, empty and then on a word, then the group's own edge.
  await editor.dblclick();
  await expect(drawer).toHaveAttribute('data-maximized', 'false');
  await page.keyboard.type('Justify');
  await editor.dblclick();
  await expect(drawer).toHaveAttribute('data-maximized', 'false');
  await expect(editor).toBeFocused();
  const box = (await group.boundingBox())!;
  await page.mouse.dblclick(box.x + 2, box.y + box.height / 2);
  await expect(drawer).toHaveAttribute('data-maximized', 'false');
  // The rail's empty space still maximizes and restores.
  const doubleClickRail = async () => {
    const rail = (await drawer.locator('.terminal-drawer__rail').boundingBox())!;
    await page.mouse.dblclick(rail.x + rail.width / 2, rail.y + rail.height - 2);
  };
  await doubleClickRail();
  await expect(drawer).toHaveAttribute('data-maximized', 'true');
  await doubleClickRail();
  await expect(drawer).toHaveAttribute('data-maximized', 'false');
});

type SearchSocketWindow = typeof window & {
  __terminalFeedbackSockets: Array<EventTarget & { url: string; readyState: number }>;
};

/** Write output to a terminal through its newest open fake socket, as the PTY would. */
async function writeTerminalOutput(page: Page, terminalId: string, output: string) {
  await page.evaluate(
    ({ terminalId, output }) => {
      const socket = (window as SearchSocketWindow).__terminalFeedbackSockets
        .filter((item) => item.url.includes(`/terminals/${terminalId}/`) && item.readyState === 1)
        .at(-1)!;
      socket.dispatchEvent(new MessageEvent('message', { data: new TextEncoder().encode(output).buffer }));
    },
    { terminalId, output },
  );
}

test('HS2-M5QPED terminal search highlights every match and counts them through edits, output, clearing, closing, and terminal switches', async ({
  page,
}) => {
  const { drawer, viewport } = await openDrawerTerminal(page);
  const group = drawer.locator('.kui-toolbar-control-group[data-content="search"]'),
    trigger = group.getByRole('button', { name: 'Search Terminal' }),
    editor = group.locator('[data-search-field-editor="terminal-search"]'),
    count = drawer.locator('[data-terminal-search-count]'),
    // xterm draws one decoration per match plus one more over the active match (addon-search 0.16 marks
    // neither with its active class), so N matches render N + 1 decorations.
    highlights = (target = viewport) => target.locator('.xterm-find-result-decoration'),
    replaceQuery = async (query: string) => {
      await editor.click();
      await page.keyboard.press('ControlOrMeta+a');
      await page.keyboard.press('Backspace');
      if (query) await page.keyboard.type(query);
    };
  await trigger.click();
  await expect(editor).toBeFocused();
  // An open, empty field shows no count.
  await expect(count).toHaveCount(0);

  // One match: highlighted as the active match, counted beside the field.
  await page.keyboard.type('Justify');
  await expect(count).toHaveText('1 of 1');
  await expect(highlights()).toHaveCount(2);
  // The count sits after the search group and before the Clear and Hide actions.
  const searchBox = (await group.boundingBox())!,
    countBox = (await count.boundingBox())!,
    clearBox = (await drawer.getByRole('button', { name: 'Clear Terminal', exact: true }).boundingBox())!;
  expect(countBox.width).toBeGreaterThan(0);
  expect(countBox.x).toBeGreaterThanOrEqual(searchBox.x + searchBox.width - 1);
  expect(countBox.x + countBox.width).toBeLessThanOrEqual(clearBox.x + 1);

  // Query edits re-count: several matches, stepped with Enter and Shift-Enter.
  await replaceQuery('File');
  await expect(count).toHaveText(/^1 of \d+$/);
  const total = Number((await count.textContent())!.split(' of ')[1]);
  expect(total).toBeGreaterThanOrEqual(2);
  await expect(highlights()).toHaveCount(total + 1);
  await page.keyboard.press('Enter');
  await expect(count).toHaveText(`2 of ${total}`);
  await page.keyboard.press('Shift+Enter');
  await expect(count).toHaveText(`1 of ${total}`);
  await expect(highlights()).toHaveCount(total + 1);
  // The count is a control too: a click steps to the next match and keeps typing in the field.
  await count.click();
  await expect(count).toHaveText(`2 of ${total}`);
  await expect(editor).toBeFocused();
  await count.click({ modifiers: ['Shift'] });
  await expect(count).toHaveText(`1 of ${total}`);

  // No match: the count says so and nothing is highlighted.
  await page.keyboard.type('-absent');
  await expect(count).toHaveText('No matches');
  await expect(count).toBeDisabled();
  await expect(highlights()).toHaveCount(0);

  // Output arriving while the search is open re-counts the matches.
  await replaceQuery('Justify');
  await expect(count).toHaveText('1 of 1');
  await writeTerminalOutput(page, 'terminal-new', '\u001b[6;1HJustify once more, Justify twice');
  await expect(count).toHaveText(/^\d of 3$/);
  await expect(highlights()).toHaveCount(4);
  await page.screenshot({ path: test.info().outputPath('hs2-m5qped-matches-1280.png') });

  // Clearing the field removes the highlights and the count; refilling brings them back.
  await group.getByRole('button', { name: 'Clear Terminal Search' }).click();
  await expect(editor).toBeFocused();
  await expect(count).toHaveCount(0);
  await expect(highlights()).toHaveCount(0);
  await page.keyboard.type('Justify');
  await expect(count).toHaveText(/^\d of 3$/);

  // Switching terminals moves the search: the left terminal's highlights go, the new one is counted.
  await drawer.locator('[data-tab-id="nano"] [data-action="select-drawer-item"]').click();
  const nano = drawer.locator('[data-component="terminal-session"]:not([hidden]) [data-terminal-id="nano"]');
  await expect(nano).toHaveAttribute('data-connection', 'connected');
  await expect(count).toHaveText('1 of 1');
  await expect(highlights(nano)).toHaveCount(2);
  await expect(highlights(viewport)).toHaveCount(0);
  await drawer.locator('[data-tab-id="terminal-new"] [data-action="select-drawer-item"]').click();
  // Back on the first terminal (its fake PTY redrew the nano screen on resume, so one match again), the
  // search follows and the second terminal's highlights go.
  await expect(highlights()).toHaveCount(2);
  await expect(count).toHaveText('1 of 1');
  await expect(highlights(nano)).toHaveCount(0);

  // Closing (empty the field, then Escape) removes the highlights and the count.
  await replaceQuery('');
  await page.keyboard.press('Escape');
  await expect(group).toHaveAttribute('data-expanded', 'false');
  await expect(count).toHaveCount(0);
  await expect(highlights()).toHaveCount(0);
});

test('HS2-HPDX81 the open search, its count, and Hide stay inside a phone-width drawer through wide-narrow-wide resizes', async ({
  page,
}) => {
  const { drawer } = await openDrawerTerminal(page);
  const group = drawer.locator('.kui-toolbar-control-group[data-content="search"]'),
    editor = group.locator('[data-search-field-editor="terminal-search"]'),
    count = drawer.locator('[data-terminal-search-count]'),
    status = drawer.locator('[data-terminal-search-count-status]'),
    hide = drawer.getByRole('button', { name: 'Hide Terminal Drawer' }),
    clear = drawer.getByRole('button', { name: 'Clear Terminal', exact: true });
  // Every listed rail control lies inside the drawer's own box (no clipping past its right edge), once
  // the shell's resize transitions have settled.
  const expectInsideDrawer = async (...controls: ReturnType<typeof page.locator>[]) => {
    await page.evaluate(() =>
      Promise.all(
        document
          .getAnimations()
          .filter((animation) => animation instanceof CSSTransition)
          .map((animation) => animation.finished.catch(() => undefined)),
      ),
    );
    const bounds = (await drawer.boundingBox())!;
    for (const control of controls) {
      const box = (await control.boundingBox())!;
      expect(box.width).toBeGreaterThan(0);
      expect(box.x).toBeGreaterThanOrEqual(bounds.x);
      expect(box.x + box.width).toBeLessThanOrEqual(bounds.x + bounds.width);
    }
  };

  // Wide: the full count control sits beside the field.
  await group.getByRole('button', { name: 'Search Terminal' }).click();
  await page.keyboard.type('File');
  await expect(count).toHaveText(/^1 of \d+$/);
  const total = Number((await count.textContent())!.split(' of ')[1]);
  await expect(status).toHaveCount(0);
  await expectInsideDrawer(group, count, clear, hide);

  // Narrow: the count yields its room; screen readers still hear it through a polite status.
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(count).toHaveCount(0);
  await expect(status).toHaveText(`1 of ${total}`);
  await expect(status).toHaveAttribute('role', 'status');
  await expect(editor).toBeFocused();
  await expectInsideDrawer(group, clear, hide);
  await page.screenshot({ path: test.info().outputPath('hs2-hpdx81-count-390.png') });
  // No matches, the state that pushed Clear and Hide out of the drawer before the fix.
  await page.keyboard.type('-absent');
  await expect(status).toHaveText('No matches');
  await expectInsideDrawer(group, clear, hide);
  await page.screenshot({ path: test.info().outputPath('hs2-hpdx81-none-390.png') });

  // Medium: the full count control returns beside the field, still inside the drawer.
  await page.setViewportSize({ width: 768, height: 900 });
  await expect(count).toHaveText('No matches');
  await expect(status).toHaveCount(0);
  await expectInsideDrawer(group, count, clear, hide);
  await page.screenshot({ path: test.info().outputPath('hs2-hpdx81-none-768.png') });

  // Wide again: the full count control returns and everything still fits.
  await page.setViewportSize({ width: 1280, height: 800 });
  await expect(count).toHaveText('No matches');
  await expect(status).toHaveCount(0);
  await expectInsideDrawer(group, count, clear, hide);
  await page.screenshot({ path: test.info().outputPath('hs2-hpdx81-none-1280.png') });
});
