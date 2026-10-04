import { expect, type Locator, type Page, test } from '@playwright/test';

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

/** Hold one finger still on `target` (DevTools touch events) for `ms`, or drag it by `dragY` px first. */
async function touchHold(page: Page, target: Locator, ms: number, dragY = 0) {
  const cdp = await page.context().newCDPSession(page),
    box = (await target.boundingBox())!,
    x = box.x + box.width / 2,
    y = box.y + box.height / 2;
  await cdp.send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: [{ x, y }] });
  for (let step = 1; step <= 5 && dragY; step += 1)
    await cdp.send('Input.dispatchTouchEvent', { type: 'touchMove', touchPoints: [{ x, y: y + (dragY * step) / 5 }] });
  await page.waitForTimeout(ms);
  await cdp.send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] });
  await cdp.detach();
  return { x, y };
}

const editMenu = (page: Page) => page.locator('[data-context-menu="terminal-edit"]');

// HS2-KKP8YJ: a still long-press on a touch terminal opens Copy Text… / Paste at the touch point.
test('opens the terminal edit menu on a long-press and copies and pastes through it', async ({ page, context }) => {
  await context.grantPermissions(['clipboard-read', 'clipboard-write']);
  await installPhone(page);
  await openProject(page);
  await page.getByRole('button', { name: 'Show terminal drawer' }).click();
  const drawer = page.locator('[data-component="terminal-drawer"]');
  await drawer.getByRole('button', { name: 'New drawer item' }).click();
  await drawer.getByRole('menuitem', { name: 'Terminal' }).click();
  const viewport = drawer.locator('[data-component="terminal-session"] [data-terminal-id="terminal-new"]');
  await expect(viewport).toHaveAttribute('data-connection', 'connected');
  await expect(viewport.locator('.xterm-rows')).toContainText('GNU nano 8.4');

  // Before focus mode, a long-press opens the menu and its lift does not also tap the terminal into focus.
  await touchHold(page, viewport, 700);
  await expect(editMenu(page).getByRole('menuitem', { name: 'Paste' })).toBeVisible();
  await expect(drawer).not.toHaveAttribute('data-focus-mode', 'true');
  await page.keyboard.press('Escape');
  await expect(editMenu(page)).toHaveCount(0);

  // A quick tap is still a tap (focus mode), and a drag scrolls rather than opening the menu.
  await touchHold(page, viewport, 100);
  await expect(drawer).toHaveAttribute('data-focus-mode', 'true');
  await expect(editMenu(page)).toHaveCount(0);
  await touchHold(page, viewport, 700, 120);
  await expect(editMenu(page)).toHaveCount(0);

  // A still hold opens the menu at the touch point, with a Lucide icon on each action.
  const point = await touchHold(page, viewport, 700);
  const menu = editMenu(page),
    copyItem = menu.getByRole('menuitem', { name: 'Copy Text…' }),
    pasteItem = menu.getByRole('menuitem', { name: 'Paste' });
  await expect(copyItem).toBeVisible();
  await expect(pasteItem).toBeVisible();
  await expect(copyItem.locator('[data-lucide="text-select"]')).toHaveCount(1);
  await expect(pasteItem.locator('[data-lucide="clipboard-paste"]')).toHaveCount(1);
  const itemBox = (await copyItem.boundingBox())!;
  expect(Math.abs(itemBox.y - point.y)).toBeLessThan(80);
  // The popup is painted in front of the focused terminal, not merely present.
  expect(
    await copyItem.evaluate((node) => {
      const box = node.getBoundingClientRect(),
        hit = document.elementFromPoint(box.left + box.width / 2, box.top + box.height / 2);
      return Boolean(hit && (hit === node || node.contains(hit) || hit.closest('[data-context-menu="terminal-edit"]')));
    }),
  ).toBe(true);
  await page.waitForTimeout(300);
  await page.screenshot({ path: test.info().outputPath('hs2-kkp8yj-edit-menu-390.png') });

  // Copy Text… opens the HS2-FRB545 copy sheet for this terminal.
  await copyItem.tap();
  await expect(menu).toHaveCount(0);
  const sheet = page.locator('[data-component="terminal-copy-dialog"]');
  await expect(sheet).toHaveJSProperty('open', true);
  await expect(sheet.getByRole('textbox', { name: 'Terminal text' })).toHaveValue(/GNU nano 8\.4/);
  await sheet.getByRole('button', { name: 'Done' }).tap();
  await expect(sheet).toHaveJSProperty('open', false);

  // Paste sends the clipboard to the terminal the menu was opened over.
  await page.evaluate(() => navigator.clipboard.writeText('echo menu'));
  await touchHold(page, viewport, 700);
  await expect(pasteItem).toBeVisible();
  const before = (await sentInput(page)).length;
  await pasteItem.tap();
  await expect(menu).toHaveCount(0);
  await expect.poll(async () => (await sentInput(page)).slice(before)).toEqual(['echo menu']);

  // Touching elsewhere dismisses the menu without acting.
  await touchHold(page, viewport, 700);
  await expect(copyItem).toBeVisible();
  await page.getByRole('button', { name: 'Exit terminal focus' }).tap();
  await expect(menu).toHaveCount(0);
  await expect(sheet).toHaveJSProperty('open', false);
});

test('long-press on a magnified phone terminal opens the edit menu without tap-focusing it', async ({ page }) => {
  await installPhone(page);
  await openProject(page);
  await page.getByRole('button', { name: 'Workspace grid' }).click();
  const dashboard = page.getByRole('region', { name: 'Workspace grid' }),
    tile = dashboard.locator('[data-terminal-key="terminal-feedback:nano"]');
  await expect(tile.locator('[data-display-mode="scaled-preview"]')).toHaveAttribute('data-geometry-ready', 'true');
  // Preview tiles are not interactive terminals: a long-press there never opens the edit menu.
  await touchHold(page, tile.locator('.terminal-tile__preview'), 700);
  await expect(editMenu(page)).toHaveCount(0);
  const magnified = dashboard.getByRole('dialog', { name: 'Magnified nano' });
  if (!(await magnified.isVisible())) await tile.click();
  const interactive = magnified.locator('[data-display-mode="interactive"]');
  await expect(interactive.locator('.xterm-rows')).toContainText('GNU nano 8.4');
  await page.locator('body').evaluate(() => {
    (document.activeElement as HTMLElement | null)?.blur();
  });
  await touchHold(page, interactive, 700);
  await expect(editMenu(page).getByRole('menuitem', { name: 'Paste' })).toBeVisible();
  // The lift that ended the long press did not also tap-focus the terminal (no keyboard).
  await expect(interactive.locator('.xterm-helper-textarea')).not.toBeFocused();
  await page.keyboard.press('Escape');
  await expect(editMenu(page)).toHaveCount(0);
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

// HS2-5DHHPV: a touch tablet in landscape uses the desktop layout but still needs Copy and Paste.
test.describe('touch tablet at a desktop width', () => {
  test.use({ viewport: { width: 1180, height: 820 }, hasTouch: true, isMobile: false, deviceScaleFactor: 2 });

  test('copies and pastes from the drawer rail, the magnified toolbar, and a long-press', async ({ page, context }) => {
    await context.grantPermissions(['clipboard-read', 'clipboard-write']);
    await installTerminalFixture(page);
    await openProject(page);
    expect(await page.evaluate(() => matchMedia('(pointer: coarse)').matches)).toBe(true);
    await page.getByRole('button', { name: 'Show terminal drawer' }).click();
    const drawer = page.locator('[data-component="terminal-drawer"]');
    await drawer.getByRole('button', { name: 'New drawer item' }).click();
    await drawer.getByRole('menuitem', { name: 'Terminal' }).click();
    const viewport = drawer.locator('[data-component="terminal-session"] [data-terminal-id="terminal-new"]');
    await expect(viewport).toHaveAttribute('data-connection', 'connected');
    // Desktop chrome: no phone focus mode, but the rail carries Copy and Paste beside Hide drawer.
    const rail = drawer.locator('.terminal-drawer__rail'),
      railCopy = rail.getByRole('button', { name: 'Copy terminal text' }),
      railPaste = rail.getByRole('button', { name: 'Paste', exact: true });
    await expect(railCopy).toBeVisible();
    await expect(railPaste).toBeVisible();
    await expect(railCopy).toHaveCSS('cursor', 'pointer');
    await expect(railCopy.locator('[data-lucide="copy"]')).toHaveCount(1);
    await expect(railPaste.locator('[data-lucide="clipboard-paste"]')).toHaveCount(1);
    const [copyBox, hideBox] = await Promise.all([
      railCopy.boundingBox(),
      rail.getByRole('button', { name: 'Hide terminal drawer' }).boundingBox(),
    ]);
    expect(copyBox!.x).toBeLessThan(hideBox!.x);
    expect(Math.abs(copyBox!.y - hideBox!.y)).toBeLessThan(2);
    await viewport.tap();
    await expect(drawer).not.toHaveAttribute('data-focus-mode', 'true');
    await page.waitForTimeout(300);
    await page.screenshot({ path: test.info().outputPath('hs2-5dhhpv-drawer-rail-1180.png') });

    await railCopy.tap();
    const sheet = page.locator('[data-component="terminal-copy-dialog"]');
    await expect(sheet).toHaveJSProperty('open', true);
    await expect(sheet.getByRole('textbox', { name: 'Terminal text' })).toHaveValue(/GNU nano 8\.4/);
    await sheet.getByRole('button', { name: 'Copy', exact: true }).tap();
    await expect(sheet).toHaveJSProperty('open', false);
    expect(await page.evaluate(() => navigator.clipboard.readText())).toContain('GNU nano 8.4');
    await page.evaluate(() => navigator.clipboard.writeText('echo tablet'));
    let before = (await sentInput(page)).length;
    await railPaste.tap();
    await expect.poll(async () => (await sentInput(page)).slice(before)).toEqual(['echo tablet']);

    // The long-press edit menu works at desktop widths too (HS2-KKP8YJ).
    await touchHold(page, viewport, 700);
    const menuPaste = editMenu(page).getByRole('menuitem', { name: 'Paste' });
    await expect(menuPaste).toBeVisible();
    before = (await sentInput(page)).length;
    await menuPaste.tap();
    await expect.poll(async () => (await sentInput(page)).slice(before)).toEqual(['echo tablet']);

    // Selecting the grid drops the rail pair; a magnified tile's desktop toolbar carries it instead.
    await drawer.getByRole('tab', { name: 'Project grid' }).tap();
    await expect(railCopy).toHaveCount(0);
    await page.getByRole('button', { name: 'Workspace grid' }).click();
    const dashboard = page.getByRole('region', { name: 'Workspace grid' }),
      tile = dashboard.locator('[data-terminal-key="terminal-feedback:nano"]');
    await expect(tile.locator('[data-display-mode="scaled-preview"]')).toHaveAttribute('data-geometry-ready', 'true');
    await expect(tile.getByRole('button', { name: 'Copy terminal text' })).toHaveCount(0);
    await tile.click();
    const magnified = dashboard.getByRole('dialog', { name: 'Magnified nano' }),
      footer = magnified.locator('.terminal-tile__footer');
    await expect(magnified.locator('[data-display-mode="interactive"]')).toHaveAttribute('data-geometry-ready', 'true');
    await expect(footer.getByRole('button', { name: 'Copy terminal text' })).toBeVisible();
    await expect(footer.getByRole('button', { name: 'Open nano in project terminal drawer' })).toBeVisible();
    expect(await footer.evaluate((node) => node.scrollWidth - node.clientWidth)).toBeLessThanOrEqual(1);
    await magnified.evaluate(async (node) => {
      await Promise.all(node.getAnimations({ subtree: true }).map((animation) => animation.finished.catch(() => 0)));
    });
    await page.screenshot({ path: test.info().outputPath('hs2-5dhhpv-magnified-1180.png') });
    await page.evaluate(() => navigator.clipboard.writeText('q'));
    before = (await sentInput(page, 'nano')).length;
    await footer.getByRole('button', { name: 'Paste' }).tap();
    await expect.poll(async () => (await sentInput(page, 'nano')).slice(before)).toEqual(['q']);
  });
});

/** The viewport rect of `text` (its first match) inside the rendered terminal rows of `viewport`. */
async function terminalTextBox(viewport: Locator, text: string) {
  const box = await viewport.evaluate((node, needle) => {
    for (const row of node.querySelectorAll('.xterm-rows > div')) {
      const content = row.textContent,
        index = content.indexOf(needle);
      if (index < 0) continue;
      const range = document.createRange(),
        walker = document.createTreeWalker(row, NodeFilter.SHOW_TEXT);
      let offset = 0,
        started = false;
      for (let current = walker.nextNode(); current; current = walker.nextNode()) {
        const length = current.textContent?.length ?? 0;
        if (!started && index < offset + length) {
          range.setStart(current, index - offset);
          started = true;
        }
        if (started && index + needle.length <= offset + length) {
          range.setEnd(current, index + needle.length - offset);
          const rect = range.getBoundingClientRect();
          return { x: rect.left, y: rect.top, width: rect.width, height: rect.height };
        }
        offset += length;
      }
    }
    return undefined;
  }, text);
  if (!box) throw new Error(`"${text}" is not rendered in the terminal`);
  return box;
}

/** Long-press at `from`, optionally keep the finger down while dragging to `to`, then lift there. */
async function touchSelect(page: Page, from: { x: number; y: number }, to?: { x: number; y: number }) {
  const cdp = await page.context().newCDPSession(page);
  await cdp.send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: [from] });
  await page.waitForTimeout(700);
  for (let step = 1; step <= 6 && to; step += 1)
    await cdp.send('Input.dispatchTouchEvent', {
      type: 'touchMove',
      touchPoints: [{ x: from.x + ((to.x - from.x) * step) / 6, y: from.y + ((to.y - from.y) * step) / 6 }],
    });
  await cdp.send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] });
  await cdp.detach();
}

// HS2-EYR96N: a long-press selects the word under the finger, dragging on extends the range, and the
// lift's edit menu copies exactly that range.
test('selects a range of terminal text with a long-press and drag, and copies it', async ({ page, context }) => {
  await context.grantPermissions(['clipboard-read', 'clipboard-write']);
  await installPhone(page);
  const { drawer, viewport } = await openFocusedDrawerTerminal(page);
  const nano = await terminalTextBox(viewport, 'nano'),
    version = await terminalTextBox(viewport, '8.4'),
    rowMiddle = nano.y + nano.height / 2,
    selection = viewport.locator('.xterm-selection div');

  // Hold still on "nano": only that word is selected, highlighted by xterm, and the lift opens the menu
  // led by Copy without tap-focusing anything.
  await touchSelect(page, { x: nano.x + nano.width / 2, y: rowMiddle });
  const menu = editMenu(page),
    copy = menu.getByRole('menuitem', { name: 'Copy', exact: true });
  await expect(copy).toBeVisible();
  await expect(copy.locator('[data-lucide="copy"]')).toHaveCount(1);
  await expect(menu.getByRole('menuitem', { name: 'Copy Text…' }).locator('[data-lucide="text-select"]')).toHaveCount(
    1,
  );
  await expect(menu.getByRole('menuitem')).toHaveText(['Copy', 'Copy Text…', 'Paste']);
  await expect(selection.first()).toBeVisible();
  await copy.tap();
  await expect(menu).toHaveCount(0);
  await expect.poll(() => page.evaluate(() => navigator.clipboard.readText())).toBe('nano');
  await expect(toast(page)).toContainText('Copied selection (1 line)');

  // Hold on "nano" again and drag to the end of "8.4": the range extends from the held word.
  await touchSelect(
    page,
    { x: nano.x + nano.width / 2, y: rowMiddle },
    { x: version.x + version.width - 2, y: rowMiddle },
  );
  await expect(copy).toBeVisible();
  const menuBox = (await menu.boundingBox())!;
  // The menu opens where the finger lifted, not where it went down.
  expect(Math.abs(menuBox.x - (version.x + version.width))).toBeLessThan(220);
  await page.waitForTimeout(300);
  await page.screenshot({ path: test.info().outputPath('hs2-eyr96n-selection-menu-390.png') });
  await page.screenshot({
    path: test.info().outputPath('hs2-eyr96n-selection-zoom-390.png'),
    clip: { x: 0, y: Math.max(0, nano.y - 12), width: 390, height: 200 },
  });
  await copy.tap();
  await expect.poll(() => page.evaluate(() => navigator.clipboard.readText())).toBe('nano 8.4');

  // Dragging backward from the held word keeps the whole word: "GNU nano" from a hold on "nano".
  const gnu = await terminalTextBox(viewport, 'GNU');
  await touchSelect(page, { x: nano.x + nano.width / 2, y: rowMiddle }, { x: gnu.x + 1, y: rowMiddle });
  await copy.tap();
  await expect.poll(() => page.evaluate(() => navigator.clipboard.readText())).toBe('GNU nano');

  // A quick tap on the terminal drops the selection; a hold on blank cells selects nothing, so the menu
  // offers only Copy Text… and Paste.
  const screen = (await viewport.locator('.xterm-screen').boundingBox())!;
  await page.touchscreen.tap(nano.x + nano.width / 2, rowMiddle);
  await expect(selection).toHaveCount(0);
  await touchSelect(page, { x: screen.x + screen.width - 4, y: screen.y + screen.height - 4 });
  await expect(menu.getByRole('menuitem')).toHaveText(['Copy Text…', 'Paste']);
  await expect(selection).toHaveCount(0);
  // An outside touch dismisses the menu and leaves focus mode as it was.
  await page.touchscreen.tap(screen.x + 8, screen.y + 8);
  await expect(menu).toHaveCount(0);
  await expect(drawer).toHaveAttribute('data-focus-mode', 'true');
});
