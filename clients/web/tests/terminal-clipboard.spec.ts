import { expect, type Page, test } from '@playwright/test';

import { installTerminalFixture } from './terminal-feedback-fixture';

// HS2-FRB545: phone terminals copy their text through a selectable sheet and paste from the clipboard,
// falling back to a paste sheet when the clipboard cannot be read.
test.use({ viewport: { width: 390, height: 844 }, hasTouch: true, isMobile: true, deviceScaleFactor: 2 });

type FeedbackWindow = typeof window & {
  __terminalFeedbackSockets: Array<{ url: string; sent: unknown[] }>;
  __setVisualViewport: (height: number) => void;
};

async function installPhone(page: Page) {
  await page.addInitScript(() => {
    const fake = Object.assign(new EventTarget(), { offsetLeft: 0, offsetTop: 0, width: 390, height: 844, scale: 1 });
    Object.defineProperty(window, 'visualViewport', { configurable: true, get: () => fake });
    (window as unknown as FeedbackWindow).__setVisualViewport = (height) => {
      fake.height = height;
      fake.dispatchEvent(new Event('resize'));
    };
  });
  await installTerminalFixture(page);
}

async function openProject(page: Page) {
  await page.goto('/');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
}

async function openFocusedDrawerTerminal(page: Page) {
  await openProject(page);
  await page.getByRole('button', { name: 'Show terminal drawer' }).click();
  const drawer = page.locator('[data-component="terminal-drawer"]');
  await drawer.getByRole('button', { name: 'New drawer item' }).click();
  await drawer.getByRole('menuitem', { name: 'Terminal' }).click();
  const viewport = drawer.locator('[data-component="terminal-session"] [data-terminal-id="terminal-new"]');
  await expect(viewport).toHaveAttribute('data-connection', 'connected');
  await expect(viewport.locator('.xterm-rows')).toContainText('GNU nano 8.4');
  await viewport.click();
  await expect(drawer).toHaveAttribute('data-focus-mode', 'true');
  return { drawer, viewport, textarea: viewport.locator('.xterm-helper-textarea') };
}

function sentInput(page: Page, terminalId = 'terminal-new') {
  return page.evaluate(
    (id) =>
      (window as unknown as FeedbackWindow).__terminalFeedbackSockets
        .filter((socket) => socket.url.includes(`/terminals/${id}/attach`))
        .flatMap((socket) => socket.sent)
        .filter((value): value is string => typeof value === 'string' && !value.startsWith('{')),
    terminalId,
  );
}

const toast = (page: Page) => page.locator('.app-toast');

test('copies all terminal text or a selection from the phone copy sheet', async ({ page, context }) => {
  await context.grantPermissions(['clipboard-read', 'clipboard-write']);
  await installPhone(page);
  const { drawer } = await openFocusedDrawerTerminal(page);
  const pill = drawer.getByRole('group', { name: 'Clipboard' });
  await expect(pill).toBeVisible();
  await expect(pill.getByRole('button', { name: 'Copy terminal text' })).toHaveCSS('cursor', 'pointer');
  await page.screenshot({ path: test.info().outputPath('hs2-frb545-focus-clipboard-pill.png') });

  await pill.getByRole('button', { name: 'Copy terminal text' }).tap();
  const sheet = page.locator('[data-component="terminal-copy-dialog"]'),
    field = sheet.getByRole('textbox', { name: 'Terminal text' });
  await expect(sheet).toHaveJSProperty('open', true);
  await expect(field).toHaveJSProperty('readOnly', true);
  const value = await field.inputValue();
  expect(value).toContain('GNU nano 8.4');
  expect(value).toContain('^X Exit');
  expect(value).not.toMatch(/\n\s*$/);
  // Opens on the newest output.
  await expect
    .poll(() => field.evaluate((node: HTMLTextAreaElement) => node.scrollHeight - node.scrollTop - node.clientHeight))
    .toBeLessThanOrEqual(2);
  await sheet.evaluate(async (node) => {
    await Promise.all(node.getAnimations({ subtree: true }).map((animation) => animation.finished.catch(() => 0)));
  });
  // Web Awesome's dialog entrance runs in its shadow root; let it settle before capturing.
  await page.waitForTimeout(400);
  await page.screenshot({ path: test.info().outputPath('hs2-frb545-copy-sheet-390.png') });

  // Nothing selected: Copy takes everything.
  await sheet.getByRole('button', { name: 'Copy', exact: true }).tap();
  await expect(sheet).toHaveJSProperty('open', false);
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(value);
  const lines = value.split('\n').length;
  await expect(toast(page)).toContainText(`Copied terminal text (${lines} lines)`);

  // A selection copies just that text; reopening snapshots afresh with no stale selection.
  await pill.getByRole('button', { name: 'Copy terminal text' }).tap();
  await expect(sheet).toHaveJSProperty('open', true);
  await expect(field).toHaveJSProperty('selectionEnd', 0);
  const start = value.indexOf('nano 8.4');
  await field.evaluate((node: HTMLTextAreaElement, from) => {
    node.setSelectionRange(from, from + 'nano 8.4'.length);
  }, start);
  await sheet.getByRole('button', { name: 'Copy', exact: true }).tap();
  await expect(sheet).toHaveJSProperty('open', false);
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe('nano 8.4');
  await expect(toast(page)).toContainText('Copied selection (1 line)');

  // Done closes without copying.
  await page.evaluate(() => navigator.clipboard.writeText('unchanged'));
  await pill.getByRole('button', { name: 'Copy terminal text' }).tap();
  await sheet.getByRole('button', { name: 'Done' }).tap();
  await expect(sheet).toHaveJSProperty('open', false);
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe('unchanged');
});

test('pastes the clipboard into the phone terminal and keeps sticky modifiers off a paste', async ({
  page,
  context,
}) => {
  await context.grantPermissions(['clipboard-read', 'clipboard-write']);
  await installPhone(page);
  const { drawer, textarea } = await openFocusedDrawerTerminal(page);
  await page.evaluate(() => navigator.clipboard.writeText('echo hi\nls'));
  let before = (await sentInput(page)).length;
  await drawer.getByRole('group', { name: 'Clipboard' }).getByRole('button', { name: 'Paste' }).tap();
  // xterm normalizes pasted newlines to carriage returns, as a desktop paste does.
  await expect.poll(async () => (await sentInput(page)).slice(before)).toEqual(['echo hi\rls']);
  await expect(page.locator('[data-component="terminal-paste-dialog"]')).toHaveJSProperty('open', false);
  await expect(textarea).toBeFocused();

  // With the keyboard up, the key bar's Fn row carries the same actions without scrolling.
  await page.evaluate(() => {
    (window as unknown as FeedbackWindow).__setVisualViewport(420);
  });
  await expect(drawer.getByRole('group', { name: 'Clipboard' })).toBeHidden();
  const keyBar = page.locator('[data-component="terminal-key-bar"]');
  await expect(keyBar).toBeVisible();
  await expect(keyBar.getByRole('button', { name: 'Paste' })).toHaveCount(0);
  await keyBar.getByRole('button', { name: 'Function and navigation keys' }).tap();
  const clipboardGroup = keyBar.getByRole('group', { name: 'Clipboard' });
  await expect(clipboardGroup).toBeVisible();
  const [groupBox, barBox] = await Promise.all([clipboardGroup.boundingBox(), keyBar.boundingBox()]);
  expect(groupBox!.x + groupBox!.width).toBeLessThanOrEqual(barBox!.x + barBox!.width);
  await page.screenshot({ path: test.info().outputPath('hs2-frb545-key-bar-clipboard.png') });
  // A sticky Ctrl applies to typed keys, never to a pasted character.
  await keyBar.getByRole('button', { name: /^Ctrl/ }).tap();
  await page.evaluate(() => navigator.clipboard.writeText('c'));
  before = (await sentInput(page)).length;
  await clipboardGroup.getByRole('button', { name: 'Paste' }).tap();
  await expect.poll(async () => (await sentInput(page)).slice(before)).toEqual(['c']);
  await expect(keyBar.getByRole('button', { name: /^Ctrl/ })).toHaveAttribute('aria-pressed', 'true');
  await expect(textarea).toBeFocused();
  // An empty clipboard sends nothing and says so.
  await page.evaluate(() => navigator.clipboard.writeText(''));
  before = (await sentInput(page)).length;
  await clipboardGroup.getByRole('button', { name: 'Paste' }).tap();
  await expect(page.locator('body')).toContainText('The clipboard is empty');
  expect((await sentInput(page)).slice(before)).toEqual([]);
});

test('falls back to a paste sheet when clipboard reading is denied or unavailable', async ({ page }) => {
  await installPhone(page);
  await page.addInitScript(() => {
    Object.defineProperty(Clipboard.prototype, 'readText', {
      configurable: true,
      value: () => Promise.reject(new DOMException('Read permission denied.', 'NotAllowedError')),
    });
  });
  const { drawer, textarea } = await openFocusedDrawerTerminal(page);
  const sheet = page.locator('[data-component="terminal-paste-dialog"]'),
    field = sheet.getByRole('textbox', { name: 'Text to paste' });
  await drawer.getByRole('group', { name: 'Clipboard' }).getByRole('button', { name: 'Paste' }).tap();
  await expect(sheet).toHaveJSProperty('open', true);
  await expect(sheet).toContainText('Clipboard access was not allowed.');
  await expect(field).toBeFocused();
  await expect(field).toHaveValue('');
  await field.fill('pwd\nwhoami');
  await sheet.evaluate(async (node) => {
    await Promise.all(node.getAnimations({ subtree: true }).map((animation) => animation.finished.catch(() => 0)));
  });
  // Web Awesome's dialog entrance runs in its shadow root; let it settle before capturing.
  await page.waitForTimeout(400);
  await page.screenshot({ path: test.info().outputPath('hs2-frb545-paste-sheet-390.png') });
  let before = (await sentInput(page)).length;
  await sheet.getByRole('button', { name: 'Paste', exact: true }).tap();
  await expect(sheet).toHaveJSProperty('open', false);
  await expect.poll(async () => (await sentInput(page)).slice(before)).toEqual(['pwd\rwhoami']);
  await expect(textarea).toBeFocused();

  // Reopening starts empty; Cancel sends nothing.
  await drawer.getByRole('group', { name: 'Clipboard' }).getByRole('button', { name: 'Paste' }).tap();
  await expect(sheet).toHaveJSProperty('open', true);
  await expect(field).toHaveValue('');
  await field.fill('discarded');
  before = (await sentInput(page)).length;
  await sheet.getByRole('button', { name: 'Cancel' }).tap();
  await expect(sheet).toHaveJSProperty('open', false);
  expect((await sentInput(page)).slice(before)).toEqual([]);

  // Without the async clipboard API at all, the sheet explains that instead.
  await page.evaluate(() => Object.defineProperty(navigator, 'clipboard', { configurable: true, value: undefined }));
  await drawer.getByRole('group', { name: 'Clipboard' }).getByRole('button', { name: 'Paste' }).tap();
  await expect(sheet).toContainText('This browser does not let Hot Sheet read the clipboard.');
  await field.fill('date');
  before = (await sentInput(page)).length;
  await field.press('Enter');
  await sheet.getByRole('button', { name: 'Paste', exact: true }).tap();
  await expect.poll(async () => (await sentInput(page)).slice(before)).toEqual(['date\r']);
});

test('copies and pastes from the phone magnified terminal toolbar', async ({ page, context }) => {
  await context.grantPermissions(['clipboard-read', 'clipboard-write']);
  await installPhone(page);
  await openProject(page);
  await page.getByRole('button', { name: 'Workspace grid' }).click();
  const dashboard = page.getByRole('region', { name: 'Workspace grid' }),
    tile = dashboard.locator('[data-terminal-key="terminal-feedback:nano"]');
  await expect(tile.locator('[data-display-mode="scaled-preview"]')).toHaveAttribute('data-geometry-ready', 'true');
  await tile.click();
  const magnified = dashboard.getByRole('dialog', { name: 'Magnified nano' }),
    footer = magnified.locator('.terminal-tile__footer');
  await expect(magnified.locator('[data-display-mode="interactive"] .xterm-rows')).toContainText('GNU nano 8.4');
  await expect(footer.getByRole('button', { name: 'Copy terminal text' })).toBeVisible();
  // Every footer control stays inside the phone width.
  const overflow = await footer.evaluate((node) => node.scrollWidth - node.clientWidth);
  expect(overflow).toBeLessThanOrEqual(1);
  // HS2-8NQRJB: the phone toolbar leaves open-in-drawer to More actions, so the identity keeps room.
  await expect(footer.getByRole('button', { name: /in project terminal drawer/ })).toHaveCount(0);
  // The terminal's own name leads untruncated, with its project as a subtitle.
  const identity = footer.locator('.terminal-tile__identity');
  await expect(identity.locator('strong')).toHaveText('Nano');
  await expect(identity.locator('.terminal-tile__identity-project')).toHaveText('Terminal feedback');
  expect(await identity.locator('strong').evaluate((node) => node.scrollWidth - node.clientWidth)).toBeLessThanOrEqual(
    0,
  );
  expect(await identity.evaluate((node) => node.clientWidth)).toBeGreaterThanOrEqual(88);
  await footer.getByRole('button', { name: 'More actions for nano' }).tap();
  const menu = page.locator('[data-context-menu="terminal"]');
  await expect(menu.getByText('Open', { exact: true })).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(menu).toHaveCount(0);
  if (!(await magnified.isVisible())) await tile.click();
  await magnified.evaluate(async (node) => {
    await Promise.all(node.getAnimations({ subtree: true }).map((animation) => animation.finished.catch(() => 0)));
  });
  await page.screenshot({ path: test.info().outputPath('hs2-frb545-magnified-toolbar-390.png') });
  await footer.getByRole('button', { name: 'Copy terminal text' }).tap();
  const sheet = page.locator('[data-component="terminal-copy-dialog"]');
  await expect(sheet.getByRole('textbox', { name: 'Terminal text' })).toHaveValue(/GNU nano 8\.4/);
  await expect(sheet).toContainText('select part of Nano');
  await sheet.getByRole('button', { name: 'Done' }).tap();
  await expect(sheet).toHaveJSProperty('open', false);
  await page.evaluate(() => navigator.clipboard.writeText('q'));
  const before = (await sentInput(page, 'nano')).length;
  await footer.getByRole('button', { name: 'Paste' }).tap();
  await expect.poll(async () => (await sentInput(page, 'nano')).slice(before)).toEqual(['q']);
});

test.describe('desktop', () => {
  test.use({ viewport: { width: 1280, height: 800 }, hasTouch: false, isMobile: false });

  test('keeps the desktop terminal free of phone clipboard controls', async ({ page }) => {
    await installTerminalFixture(page);
    await openProject(page);
    await page.getByRole('button', { name: 'Show terminal drawer' }).click();
    const drawer = page.locator('[data-component="terminal-drawer"]');
    await drawer.getByRole('button', { name: 'New drawer item' }).click();
    await drawer.getByRole('menuitem', { name: 'Terminal' }).click();
    const viewport = drawer.locator('[data-component="terminal-session"] [data-terminal-id="terminal-new"]');
    await expect(viewport).toHaveAttribute('data-connection', 'connected');
    await viewport.click();
    await expect(drawer).not.toHaveAttribute('data-focus-mode', 'true');
    await expect(page.locator('[data-action="copy-terminal-text"], [data-action="paste-terminal-text"]')).toHaveCount(
      0,
    );
    await expect(page.locator('[data-component="terminal-copy-dialog"]')).toHaveJSProperty('open', false);
    await page.screenshot({ path: test.info().outputPath('hs2-frb545-desktop-1280.png') });
  });
});
