import { expect, test } from '@playwright/test';

import { KEYBOARD_SHORTCUTS, shortcutDef } from '../src/keyboard-shortcuts';
import { appRequests } from './app-requests';

// End-to-end coverage for the view/panel and tab-cycling keyboard shortcuts (HS2-9SHYWD). `ControlOrMeta`
// maps to Cmd on macOS and Ctrl elsewhere — exactly the app's `mod` modifier — so the same presses
// exercise the correct chord on either host.
const capabilities = {
  create: true,
  update: true,
  close: true,
  notes: true,
  note_edit: true,
  note_delete: true,
  attachments: true,
  assignment: true,
  review_requests: true,
  dependencies: true,
  up_next: true,
  close_reasons: true,
  claims: true,
  atomic_batch: true,
  not_working_report: true,
  offline_mutation: true,
  history: true,
  watch: true,
  provider_idempotency: true,
  query_fields: [],
};
const project = (id: string, root: string) => ({
  id,
  root,
  name: root.split('/').at(-1),
  stores: [`${root}.hs2`],
  apiPath: `/__hotsheet/project-api/${id}`,
});
const ticket = (slug: string, status: string) => ({
  connection_id: 'git-local',
  native_id: slug,
  qualified_id: `git-local:${slug}`,
  id: slug,
  slug,
  title: slug,
  status,
  up_next: false,
  feedback_needed: false,
  tags: [],
  blocked_by: [],
  claim_count: 0,
});
const counts = {
  total: 1,
  queued: 1,
  backlog: 0,
  archive: 0,
  open: 1,
  up_next: 0,
  active: 0,
  started: 1,
  completed_today: 0,
  completion_trend: [0, 0, 0, 0, 0, 0, 0],
};

async function installFixture(page: import('@playwright/test').Page) {
  await page.route(appRequests, (route) => {
    const request = route.request(),
      url = new URL(request.url()),
      path = url.pathname,
      other = path.includes('other-checkout');
    if (/^\/__hotsheet\/(?:remote\/)?projects\/open$/.test(path)) {
      const root = request.postDataJSON().root as string;
      return route.fulfill({
        status: 201,
        json: root === '/work/other' ? project('other-checkout', root) : project('demo-checkout', root),
      });
    }
    if (path === '/__hotsheet/folders/choose') return route.fulfill({ json: { path: '/work/other' } });
    if (path.endsWith('/providers'))
      return route.fulfill({
        json: [
          {
            connection_id: 'git-local',
            provider: 'git',
            display_name: 'Hot Sheet Git',
            locator: '/tickets',
            default: true,
            capabilities,
          },
        ],
      });
    if (path.endsWith('/tickets'))
      return route.fulfill({ json: { items: [ticket(other ? 'HS2-OTHER1' : 'HS2-DEMO1', 'started')], counts } });
    // One ticket's detail, for the reader the minimize chord takes off the modal stack (HS2-Q8WF64).
    const detail = /\/tickets\/([^/]+)$/.exec(path)?.[1];
    if (detail && request.method() === 'GET') {
      const slug = decodeURIComponent(detail).split(':').at(-1) ?? detail;
      return route.fulfill({
        json: {
          ...ticket(slug, 'started'),
          store: 'git-local',
          category: 'task',
          priority: 'default',
          created_at: '2026-11-01T00:00:00Z',
          updated_at: '2026-11-01T00:00:00Z',
          details: '',
          blocked_reason: null,
          concurrency_token: `token-${slug}`,
          notes: [],
          attachments: [],
        },
      });
    }
    if (path.endsWith('/repository/status'))
      return route.fulfill({
        json: { branch: 'main', ahead: 0, behind: 0, staged: 0, unstaged: 0, untracked: 0, conflicted: 0, clean: true },
      });
    if (path.endsWith('/ws/poll'))
      return route.fulfill({
        json: { cursor: Number(url.searchParams.get('since') ?? 0), events: [], overflow: false },
      });
    if (
      path.endsWith('/terminals') ||
      path.endsWith('/connections') ||
      path.endsWith('/commands') ||
      path.endsWith('/command-runs') ||
      path.endsWith('/views') ||
      path.endsWith('/corrupt-tickets')
    )
      return route.fulfill({ json: [] });
    return route.continue();
  });
}

test('drives views, panels, tab cycling, and the composer from the keyboard (HS2-9SHYWD)', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await installFixture(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open Project' }).click();
  await page.getByRole('button', { name: 'Local Project' }).click();
  await page.locator('input[name="project-root"]').evaluate((node: HTMLElement & { value: string }, value) => {
    node.value = value;
  }, '/work/hotsheet2');
  await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();

  const list = page.locator('[data-component="ticket-list"]');
  const board = page.locator('[data-component="ticket-board"]');
  const appShell = page.locator('[data-component="app-shell"]');
  const inspectorRegion = appShell.locator('#app-right-rail');
  const drawer = page.locator('[data-component="terminal-drawer"]');
  const composer = page.locator('[data-component="quick-ticket-composer"]');
  await expect(list).toBeVisible();
  // Anchor focus on a stable, non-editable toolbar control (no-op: already the list view) so the first
  // keydown dispatches from an attached node and reaches the document-level dispatcher.
  await page.getByRole('button', { name: 'List View' }).click();

  // Column (board) ↔ list view.
  await page.keyboard.press('ControlOrMeta+Shift+B');
  await expect(board).toBeVisible();
  await expect(list).toHaveCount(0);
  await page.keyboard.press('ControlOrMeta+Shift+L');
  await expect(list).toBeVisible();
  await expect(board).toHaveCount(0);

  // Left sidebar toggle (the sidebar region collapses; the app-shell records the state).
  await expect(appShell).toHaveAttribute('data-sidebar-visible', 'true');
  await page.keyboard.press('ControlOrMeta+b');
  await expect(appShell).toHaveAttribute('data-sidebar-visible', 'false');
  await page.keyboard.press('ControlOrMeta+b');
  await expect(appShell).toHaveAttribute('data-sidebar-visible', 'true');

  // Bottom drawer toggle.
  await expect(drawer).toHaveCount(0);
  await page.keyboard.press('ControlOrMeta+j');
  await expect(drawer).toBeVisible();
  await page.keyboard.press('ControlOrMeta+j');
  await expect(page.getByRole('button', { name: 'Show Terminal Drawer' })).toBeVisible();

  // The right-sidebar default adds Shift so Safari's ⌘⌥B bookmark editor keeps its chord.
  await expect(inspectorRegion).toHaveAttribute('data-collapsed', 'false');
  await page.keyboard.press('ControlOrMeta+Alt+Shift+B');
  await expect(inspectorRegion).toHaveAttribute('data-collapsed', 'true');
  await page.keyboard.press('ControlOrMeta+Alt+Shift+B');
  await expect(inspectorRegion).toHaveAttribute('data-collapsed', 'false');

  // Project-tab cycling with two open projects uses shifted chords so browsers retain their
  // unshifted ⌘/Ctrl+⌥+Arrow tab navigation.
  await page.getByRole('button', { name: 'Add Project' }).click();
  await page.getByRole('button', { name: 'Local Project' }).click();
  await expect(page.getByRole('tab', { name: 'other' })).toHaveAttribute('aria-selected', 'true');
  // The Local or Remote Project choice must finish closing and return focus before tab-cycling chords (HS2-BT4KN6).
  await expect(page.locator('[data-project-source-choice]')).toHaveJSProperty('open', false);
  await expect(page.locator('[data-project-source-choice] :focus')).toHaveCount(0);
  await page.keyboard.press('ControlOrMeta+Alt+Shift+ArrowLeft');
  await expect(page.getByRole('tab', { name: 'hotsheet2' })).toHaveAttribute('aria-selected', 'true');
  await page.keyboard.press('ControlOrMeta+Alt+Shift+ArrowRight');
  await expect(page.getByRole('tab', { name: 'other' })).toHaveAttribute('aria-selected', 'true');

  // Settings uses a mnemonic chord instead of Safari's application-settings shortcut (⌘,).
  await page.keyboard.press('ControlOrMeta+Alt+S');
  await expect(page.getByRole('navigation', { name: 'Settings categories' })).toBeVisible();
  await page.getByRole('button', { name: 'Keyboard' }).click();
  const keyboardSettings = page.locator('[data-component="keyboard-settings"]');
  await expect(keyboardSettings.locator('li[data-shortcut-id="toggle-right-sidebar"]')).toContainText(
    /(Ctrl\+Alt\+Shift\+B|⌘⌥⇧B)/,
  );
  await expect(keyboardSettings.locator('li[data-shortcut-id="view-settings"]')).toContainText(/(Ctrl\+Alt\+S|⌘⌥S)/);
  await expect(keyboardSettings.locator('li[data-shortcut-id="project-tab-previous"]')).toContainText(
    /(Ctrl\+Alt\+Shift\+Left|⌘⌥⇧←)/,
  );
  await page.screenshot({ path: 'target/visual-captures/hs2-q1bh0v-browser-safe-shortcuts-wide.png', fullPage: true });
  await keyboardSettings
    .locator('li[data-shortcut-id="toggle-right-sidebar"]')
    .screenshot({ path: 'target/visual-captures/hs2-q1bh0v-right-sidebar-shortcut-after.png' });
  await keyboardSettings
    .locator('li[data-shortcut-id="view-settings"]')
    .screenshot({ path: 'target/visual-captures/hs2-q1bh0v-settings-shortcut-after.png' });
  const firstTabDefault = keyboardSettings.locator('li[data-shortcut-id="project-tab-previous"]');
  const lastTabDefault = keyboardSettings.locator('li[data-shortcut-id="drawer-tab-next"]');
  const captureTabDefaults = async (path: string) => {
    await firstTabDefault.scrollIntoViewIfNeeded();
    const firstBox = await firstTabDefault.boundingBox(),
      lastBox = await lastTabDefault.boundingBox();
    if (!firstBox || !lastBox) throw new Error('Browser-safe tab shortcut rows were not measurable');
    await page.screenshot({
      path,
      clip: { x: firstBox.x, y: firstBox.y, width: firstBox.width, height: lastBox.y + lastBox.height - firstBox.y },
    });
  };
  await captureTabDefaults('target/visual-captures/hs2-q1bh0v-navigation-shortcuts-after-wide.png');
  await page.setViewportSize({ width: 760, height: 700 });
  await captureTabDefaults('target/visual-captures/hs2-q1bh0v-navigation-shortcuts-after-narrow.png');
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.locator('.view-mode-switcher [aria-label="List View"]').click();

  // Bare `c` opens the new-ticket composer; Escape closes it.
  await expect(composer).toBeHidden();
  await page.keyboard.press('c');
  await expect(composer).toBeVisible();
  await page.screenshot({ path: 'target/visual-captures/hs2-9shywd-shortcuts-wide.png', fullPage: true });
  await page.keyboard.press('Escape');
  await expect(composer).toBeHidden();

  // The bare `c` must NOT fire while a text field is focused: open search, type into it, and confirm
  // the composer stays closed (the character is typed, not consumed as a shortcut).
  await page.getByRole('button', { name: 'Search tickets' }).click();
  const search = page.locator('[data-search-field-editor="workspace-search"]');
  await expect(search).toBeFocused();
  await page.keyboard.press('c');
  await expect(composer).toBeHidden();
});

// The trusted keydown a macOS keyboard sends for a chord (US layout). Option composes `key` even with
// Command held (⌥S → `ß`, ⌥⇧B → `ı`); `code` names the physical key. Playwright's `keyboard.press`
// always reports the base letter, which is why the HS2-9SHYWD flow above passed while ⌘⌥S and ⌘⌥⇧B
// did nothing on a real Mac (HS2-F0BT8E). CDP sends the composed values Chromium receives from macOS.
const macOption: Record<string, string> = { s: 'ß', b: '∫' };
const macShiftOption: Record<string, string> = { b: 'ı', s: 'Í' };
const arrowCodes: Record<string, number> = { ArrowLeft: 37, ArrowUp: 38, ArrowRight: 39, ArrowDown: 40 };
async function pressMacChord(
  cdp: import('@playwright/test').CDPSession,
  chord: { key: string; mod?: boolean; shift?: boolean; alt?: boolean },
) {
  const letter = /^[a-z]$/.test(chord.key);
  let key = letter && chord.shift ? chord.key.toUpperCase() : chord.key;
  if (letter && chord.alt) key = (chord.shift ? macShiftOption : macOption)[chord.key] ?? key;
  const modifiers = (chord.alt ? 1 : 0) | (chord.mod ? 4 : 0) | (chord.shift ? 8 : 0);
  const code = letter ? `Key${chord.key.toUpperCase()}` : chord.key;
  const windowsVirtualKeyCode = letter ? chord.key.toUpperCase().charCodeAt(0) : (arrowCodes[chord.key] ?? 0);
  // A Command or Option chord produces no text input on macOS, so a raw keydown is faithful.
  const plain = !chord.mod && !chord.alt;
  await cdp.send('Input.dispatchKeyEvent', {
    type: plain ? 'keyDown' : 'rawKeyDown',
    key,
    code,
    modifiers,
    windowsVirtualKeyCode,
    ...(plain ? { text: key } : {}),
  });
  await cdp.send('Input.dispatchKeyEvent', { type: 'keyUp', key, code, modifiers, windowsVirtualKeyCode });
}

test('every editable default command shortcut reaches its handler from a macOS keyboard (HS2-F0BT8E)', async ({
  page,
  browserName,
}) => {
  test.skip(browserName !== 'chromium', 'CDP key events are Chromium-only');
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.addInitScript(() => {
    Object.defineProperty(navigator, 'userAgent', { get: () => 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)' });
  });
  await installFixture(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Open Project' }).click();
  await page.getByRole('button', { name: 'Local Project' }).click();
  await page.locator('input[name="project-root"]').evaluate((node: HTMLElement & { value: string }) => {
    node.value = '/work/hotsheet2';
  });
  await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();
  const cdp = await page.context().newCDPSession(page);
  const pressed = new Set<string>();
  const press = (id: string) => {
    const shortcut = shortcutDef(id);
    if (!shortcut) throw new Error(`Unknown shortcut ${id}`);
    pressed.add(id);
    return pressMacChord(cdp, shortcut.defaultChord);
  };

  const list = page.locator('[data-component="ticket-list"]');
  const board = page.locator('[data-component="ticket-board"]');
  const appShell = page.locator('[data-component="app-shell"]');
  const inspectorRegion = appShell.locator('#app-right-rail');
  const drawer = page.locator('[data-component="terminal-drawer"]');
  const composer = page.locator('[data-component="quick-ticket-composer"]');
  const workspaceGrid = page.getByRole('button', { name: 'Workspace Grid', exact: true });
  const allStats = page.getByRole('button', { name: 'Cross-Project Stats' });
  const search = page.getByRole('searchbox', { name: 'Search tickets' });
  await expect(list).toBeVisible();
  await page.getByRole('button', { name: 'List View' }).click();

  // Every chord below uses the event values a Mac keyboard really produces.
  await press('view-settings');
  await expect(page.getByRole('navigation', { name: 'Settings categories' })).toBeVisible();
  // After capture of the rows the ticket reported (reached through the Mac ⌘⌥S chord itself).
  await page.locator('#app-left-rail [data-item-id="keyboard"]').click();
  const keyboardRow = (id: string) => page.locator(`[data-component="keyboard-settings"] li[data-shortcut-id="${id}"]`);
  await expect(keyboardRow('view-settings')).toContainText('⌘⌥S');
  await expect(keyboardRow('toggle-right-sidebar')).toContainText('⌘⌥⇧B');
  await keyboardRow('view-settings').screenshot({ path: 'target/visual-captures/hs2-f0bt8e-settings-view-row.png' });
  await keyboardRow('toggle-right-sidebar').screenshot({
    path: 'target/visual-captures/hs2-f0bt8e-right-sidebar-row.png',
  });
  await press('view-notifications');
  await expect(page.getByRole('navigation', { name: 'Settings categories' })).toHaveCount(0);
  await press('view-board');
  await expect(board).toBeVisible();
  await press('view-list');
  await expect(list).toBeVisible();

  await expect(inspectorRegion).toHaveAttribute('data-collapsed', 'false');
  await press('toggle-right-sidebar');
  await expect(inspectorRegion).toHaveAttribute('data-collapsed', 'true');
  await press('toggle-right-sidebar');
  await expect(inspectorRegion).toHaveAttribute('data-collapsed', 'false');
  await press('toggle-left-sidebar');
  await expect(appShell).toHaveAttribute('data-sidebar-visible', 'false');
  await press('toggle-left-sidebar');
  await expect(appShell).toHaveAttribute('data-sidebar-visible', 'true');
  await press('toggle-bottom-drawer');
  await expect(drawer).toBeVisible();
  await press('toggle-bottom-drawer');
  await expect(drawer).toHaveCount(0);
  await press('drawer-tab-next');
  await expect(drawer).toBeVisible();
  await press('toggle-bottom-drawer');
  await expect(drawer).toHaveCount(0);
  await press('drawer-tab-previous');
  await expect(drawer).toBeVisible();
  await press('toggle-bottom-drawer');
  await expect(drawer).toHaveCount(0);

  // The global dashboards toggle, and a view chord returns from them to the project (HS2-76ZR5P note).
  await press('view-workspace-grid');
  await expect(workspaceGrid).toHaveAttribute('aria-pressed', 'true');
  await press('view-list');
  await expect(workspaceGrid).toHaveAttribute('aria-pressed', 'false');
  await expect(list).toBeVisible();
  await press('view-all-stats');
  await expect(allStats).toHaveAttribute('aria-pressed', 'true');
  await press('view-all-stats');
  await expect(allStats).toHaveAttribute('aria-pressed', 'false');

  await press('open-search');
  await expect(search).toBeFocused();
  await page.keyboard.press('Escape');
  await expect(search).toHaveCount(0);
  await page.locator('.app-shell__work-area').focus();
  await press('new-ticket');
  await expect(composer).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(composer).toBeHidden();

  // The minimize chord takes the open reader off the modal stack and brings it back (HS2-Q8WF64).
  await page.locator('[data-component="summary-row"][data-ticket-slug="HS2-DEMO1"]').dblclick();
  const reader = page.getByRole('dialog', { name: /^Read and edit HS2-DEMO1 in / });
  await expect(reader).toBeVisible();
  await press('minimize-ticket-reader');
  await expect(reader).toHaveCount(0);
  await expect(page.locator('[data-minimized-tickets-control]')).toBeVisible();
  await press('minimize-ticket-reader');
  await expect(page.getByRole('dialog', { name: /^Read and edit HS2-DEMO1 in / })).toBeVisible();
  await expect(page.locator('[data-minimized-tickets-control]')).toHaveCount(0);
  await page
    .getByRole('dialog', { name: /^Read and edit HS2-DEMO1 in / })
    .getByRole('button', { name: 'Close Ticket Reader' })
    .click();
  await expect(page.locator('dialog[data-reader-dialog][open]')).toHaveCount(0);

  await page.getByRole('button', { name: 'Add Project' }).click();
  await page.getByRole('button', { name: 'Local Project' }).click();
  await expect(page.getByRole('tab', { name: 'other' })).toHaveAttribute('aria-selected', 'true');
  await expect(page.locator('[data-project-source-choice]')).toHaveJSProperty('open', false);
  await press('project-tab-previous');
  await expect(page.getByRole('tab', { name: 'hotsheet2' })).toHaveAttribute('aria-selected', 'true');
  await press('project-tab-next');
  await expect(page.getByRole('tab', { name: 'other' })).toHaveAttribute('aria-selected', 'true');

  // Every editable command chord was exercised, apart from the ticket history and clipboard chords that
  // need ticket state; their plain-letter defaults are covered by the history and clipboard specs.
  const historyAndClipboard = ['undo', 'redo', 'select-all-tickets', 'copy-tickets', 'cut-tickets', 'paste-tickets'];
  expect(
    KEYBOARD_SHORTCUTS.filter((shortcut) => shortcut.editable && !historyAndClipboard.includes(shortcut.id))
      .map((shortcut) => shortcut.id)
      .filter((id) => !pressed.has(id)),
  ).toEqual([]);
});

for (const apple of [true, false]) {
  test(`captures physical Control chords in settings on ${apple ? 'Apple' : 'non-Apple'} (HS2-835BZD)`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.addInitScript((isApple) => {
      Object.defineProperty(navigator, 'userAgent', {
        get: () =>
          isApple ? 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)' : 'Mozilla/5.0 (Windows NT 10.0; Win64; x64)',
      });
    }, apple);
    await installFixture(page);
    // Keep the real sync client connected without a zero-delay polling fallback in this HTTP fixture.
    await page.routeWebSocket(/\/__hotsheet\/project-api\/[^/]+\/ws\/sync(?:\?.*)?$/, () => undefined);
    await page.goto('/');
    await page.getByRole('button', { name: 'Open Project' }).click();
    await page.getByRole('button', { name: 'Local Project' }).click();
    await page.locator('input[name="project-root"]').evaluate((node: HTMLElement & { value: string }) => {
      node.value = '/work/hotsheet2';
    });
    await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();
    await page.getByLabel('Settings View').click();
    await page.locator('#app-left-rail [data-item-id="keyboard"]').click();
    const screen = page.locator('[data-component="keyboard-settings"]');
    const row = (id: string) => screen.locator(`li[data-shortcut-id="${id}"]`);
    const record = async (id: string) => {
      await row(id).locator('[data-action="edit-shortcut"]').click();
      await expect(screen.locator(`[data-shortcut-capture="${id}"]`)).toBeFocused();
    };
    const stored = () => page.evaluate(() => JSON.parse(localStorage.getItem('hotsheet.keyboard-shortcuts') || '{}'));

    await record('open-search');
    await page.keyboard.down('Control');
    await expect(screen.locator('[data-shortcut-capture="open-search"]')).toBeFocused();
    expect(await stored()).toEqual({});
    await page.keyboard.press('g');
    await page.keyboard.up('Control');
    await expect(row('open-search').locator('kbd')).toHaveText(apple ? '⌃G' : 'Ctrl+G');
    expect(await stored()).toEqual({
      'open-search': { key: 'g', mod: !apple, shift: false, alt: false, ...(apple ? { ctrl: true } : {}) },
    });
    await expect(screen.locator('[data-shortcut-capture]')).toHaveCount(0);
    await expect(page.getByRole('searchbox', { name: 'Search tickets' })).toHaveCount(0);

    // Escape and the explicit Cancel action leave the previous binding intact, even with Control held.
    const saved = await stored();
    await record('open-search');
    await page.keyboard.press('Control+Escape');
    expect(await stored()).toEqual(saved);
    await record('undo');
    await page.keyboard.down('Control');
    await page.keyboard.up('Control');
    await screen.getByRole('button', { name: 'Cancel Editing Undo', exact: true }).click();
    expect(await stored()).toEqual(saved);

    // Different physical modifiers stay distinct; adding the exact same chord flags both rows.
    await record('undo');
    await page.keyboard.press('Meta+g');
    await expect(row('undo').locator('kbd')).toHaveText(apple ? '⌘G' : 'Meta+G');
    await expect(screen.locator('.keyboard-settings__conflict')).toHaveCount(0);
    await record('undo');
    await page.keyboard.press('Control+g');
    await expect(row('undo').getByRole('status')).toContainText('Open Search');
    await expect(row('open-search').getByRole('status')).toContainText('Undo');
    if (apple) await page.screenshot({ path: 'target/visual-captures/hs2-835bzd-control-conflict-wide.png' });
    await row('undo').locator('[data-action="reset-shortcut"]').click();
    await expect(screen.locator('.keyboard-settings__conflict')).toHaveCount(0);

    // Every held modifier survives the editor and reload. No subset can invoke the binding.
    await record('redo');
    await page.keyboard.press('Control+Meta+Alt+Shift+J');
    await expect(row('redo').locator('kbd')).toHaveText(apple ? '⌃⌘⌥⇧J' : 'Ctrl+Meta+Alt+Shift+J');
    await page.reload();
    await page.getByLabel('Settings View').click();
    await page.locator('#app-left-rail [data-item-id="keyboard"]').click();
    await expect(row('open-search').locator('kbd')).toHaveText(apple ? '⌃G' : 'Ctrl+G');
    await expect(row('redo').locator('kbd')).toHaveText(apple ? '⌃⌘⌥⇧J' : 'Ctrl+Meta+Alt+Shift+J');

    await page.getByRole('button', { name: 'List View', exact: true }).click();
    await page.locator('.app-shell__work-area').focus();
    await page.keyboard.press('g');
    await page.keyboard.press('Meta+g');
    await page.keyboard.press('Control+Alt+g');
    await page.keyboard.press('Control+Meta+g');
    await expect(page.getByRole('searchbox', { name: 'Search tickets' })).toHaveCount(0);
    await page.keyboard.press('Control+g');
    await expect(page.getByRole('searchbox', { name: 'Search tickets' })).toBeFocused();
    await page.keyboard.press('Escape');
    await expect(page.getByRole('searchbox', { name: 'Search tickets' })).toHaveCount(0);

    await page.getByLabel('Settings View').click();
    await screen.getByRole('button', { name: 'Reset all to Defaults' }).click();
    expect(await stored()).toEqual({});
    await expect(row('open-search').locator('kbd')).toHaveText(apple ? '⌘K' : 'Ctrl+K');
    await expect(screen.getByRole('button', { name: 'Reset all to Defaults' })).toBeDisabled();
    await record('open-search');
    await page.keyboard.press('Control+q');
    await expect(row('open-search').locator('kbd')).toHaveText(apple ? '⌃Q' : 'Ctrl+Q');
    await page.setViewportSize({ width: 390, height: 844 });
    await expect(page.locator('[data-component="app-shell"]')).toHaveAttribute('data-mobile', 'true');
    await expect
      .poll(async () =>
        page.locator('.app-shell__main').evaluate((node) => {
          const box = node.getBoundingClientRect();
          return { x: Math.round(box.x), width: Math.round(box.width) };
        }),
      )
      .toEqual({ x: 0, width: 390 });
    await expect(row('open-search')).toBeVisible();
    await record('undo');
    await page.keyboard.press('Control+q');
    await expect(row('undo').getByRole('status')).toContainText('Open Search');
    await expect(row('open-search').getByRole('status')).toContainText('Undo');
    const bounds = await screen.evaluate((node) => ({
      width: node.getBoundingClientRect().width,
      scroll: node.scrollWidth,
    }));
    expect(bounds.scroll).toBeLessThanOrEqual(bounds.width + 1);
    if (apple) {
      await page.screenshot({
        path: 'target/visual-captures/hs2-835bzd-control-conflict-narrow.png',
        animations: 'disabled',
      });
    }
    await row('undo').locator('[data-action="reset-shortcut"]').click();
    await expect(screen.locator('.keyboard-settings__conflict')).toHaveCount(0);
  });
}
