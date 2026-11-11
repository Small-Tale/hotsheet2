import { expect, type Page, test } from '@playwright/test';

import { installTerminalFixture } from './terminal-feedback-fixture';

// HS2-N2AG8V: the drawer's Clear Terminal button and Command-K clear the terminal display, like
// Command-K in macOS Terminal, without sending anything to the shell.
test.use({ viewport: { width: 1280, height: 800 } });

type FeedbackWindow = typeof window & { __terminalFeedbackSockets: Array<{ url: string; sent: unknown[] }> };

async function openDrawerTerminal(page: Page) {
  await installTerminalFixture(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open Project' }).click();
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

/** Typed input the page sent to the terminal (control frames are JSON). */
function sentInput(page: Page) {
  return page.evaluate(() =>
    (window as unknown as FeedbackWindow).__terminalFeedbackSockets
      .filter((socket) => socket.url.includes('/terminals/terminal-new/attach'))
      .flatMap((socket) => socket.sent)
      .filter((value): value is string => typeof value === 'string' && !value.startsWith('{')),
  );
}

/** Size frames the page sent; the fixture redraws its full-screen program on each one. */
function sentResizes(page: Page) {
  return page.evaluate(
    () =>
      (window as unknown as FeedbackWindow).__terminalFeedbackSockets
        .filter((socket) => socket.url.includes('/terminals/terminal-new/attach'))
        .flatMap((socket) => socket.sent)
        .filter((value) => typeof value === 'string' && value.includes('"resize"')).length,
  );
}

/** Focus the terminal and wait until its focus-driven size claims stop, so no redraw follows a clear. */
async function focusSettled(page: Page, viewport: import('@playwright/test').Locator) {
  await viewport.click();
  await expect(viewport.locator('.xterm-helper-textarea')).toBeFocused();
  let last = -1;
  await expect
    .poll(async () => {
      const count = await sentResizes(page),
        settled = count === last;
      last = count;
      return settled;
    })
    .toBe(true);
}

const workspaceSearchFocused = (page: Page) =>
  page.evaluate(() => Boolean(document.activeElement?.closest('[data-search-field-editor="workspace-search"]')));

test('HS2-N2AG8V Clear Terminal in the drawer rail clears the selected terminal and keeps its focus', async ({
  page,
}) => {
  const { drawer, viewport } = await openDrawerTerminal(page);
  const actions = drawer.locator('.terminal-drawer__actions');
  const clear = actions.getByRole('button', { name: 'Clear Terminal' });
  // The button sits beside Hide in the rail's borderless end group.
  await expect(actions.getByRole('button')).toHaveText(['', '']);
  await expect(actions.locator('button').last()).toHaveAttribute('aria-label', 'Hide Terminal Drawer');
  await expect(clear.locator('[data-lucide="eraser"]')).toBeVisible();
  await focusSettled(page, viewport);
  const before = await sentInput(page);

  await clear.click();
  await expect(viewport.locator('.xterm-rows')).not.toContainText('GNU nano 8.4');
  await expect(viewport.locator('.xterm-helper-textarea')).toBeFocused();
  // A view action: the shell receives nothing.
  expect(await sentInput(page)).toEqual(before);
  await page.screenshot({ path: test.info().outputPath('hs2-n2ag8v-cleared-1280.png') });

  // The grid view has no single terminal to clear.
  await drawer.getByRole('button', { name: 'Project Grid' }).click();
  await expect(drawer.locator('[data-action="clear-terminal"]')).toHaveCount(0);
});

test('HS2-N2AG8V Command-K clears a focused terminal instead of opening search; elsewhere it still opens search', async ({
  page,
}) => {
  const { viewport } = await openDrawerTerminal(page);
  const textarea = viewport.locator('.xterm-helper-textarea');
  await focusSettled(page, viewport);
  const before = await sentInput(page);

  await page.keyboard.press('Meta+k');
  await expect(viewport.locator('.xterm-rows')).not.toContainText('GNU nano 8.4');
  await expect(textarea).toBeFocused();
  expect(await workspaceSearchFocused(page)).toBe(false);
  expect(await sentInput(page)).toEqual(before);
  // A repeated clear is harmless.
  await page.keyboard.press('Meta+k');
  await expect(textarea).toBeFocused();
  expect(await sentInput(page)).toEqual(before);

  // Outside the terminal, the platform's Open Search chord still works.
  await textarea.blur();
  await page.locator('body').click({ position: { x: 640, y: 120 } });
  await page.keyboard.press('ControlOrMeta+k');
  await expect.poll(() => workspaceSearchFocused(page)).toBe(true);
});
