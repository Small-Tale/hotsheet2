import { expect, test } from '@playwright/test';

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
  await page.route('**/*', (route) => {
    const request = route.request(),
      url = new URL(request.url()),
      path = url.pathname,
      other = path.includes('other-checkout');
    if (path === '/__hotsheet/projects/open') {
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
            display_name: 'Hot Sheet git',
            locator: '/tickets',
            default: true,
            capabilities,
          },
        ],
      });
    if (path.endsWith('/tickets'))
      return route.fulfill({ json: { items: [ticket(other ? 'HS2-OTHER1' : 'HS2-DEMO1', 'started')], counts } });
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
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.locator('wa-input[name="project-root"]').evaluate((node: HTMLElement & { value: string }, value) => {
    node.value = value;
  }, '/work/hotsheet2');
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();

  const list = page.locator('[data-component="ticket-list"]');
  const board = page.locator('[data-component="ticket-board"]');
  const appShell = page.locator('[data-component="app-shell"]');
  const inspectorRegion = appShell.locator('[data-component="resizable-region"][data-region-id="app-inspector"]');
  const drawer = page.locator('[data-component="terminal-drawer"]');
  const composer = page.locator('[data-component="quick-ticket-composer"]');
  await expect(list).toBeVisible();
  // Anchor focus on a stable, non-editable toolbar control (no-op: already the list view) so the first
  // keydown dispatches from an attached node and reaches the document-level dispatcher.
  await page.getByRole('button', { name: 'List view' }).click();

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
  await expect(page.getByRole('button', { name: 'Show terminal drawer' })).toBeVisible();

  // The right-sidebar default adds Shift so Safari's ⌘⌥B bookmark editor keeps its chord.
  await expect(inspectorRegion).toHaveAttribute('data-collapsed', 'false');
  await page.keyboard.press('ControlOrMeta+Alt+Shift+B');
  await expect(inspectorRegion).toHaveAttribute('data-collapsed', 'true');
  await page.keyboard.press('ControlOrMeta+Alt+Shift+B');
  await expect(inspectorRegion).toHaveAttribute('data-collapsed', 'false');

  // Project-tab cycling with two open projects uses shifted chords so browsers retain their
  // unshifted ⌘/Ctrl+⌥+Arrow tab navigation.
  await page.getByRole('button', { name: 'Add project' }).click();
  await expect(page.getByRole('tab', { name: 'other' })).toHaveAttribute('aria-selected', 'true');
  await page.keyboard.press('ControlOrMeta+Alt+Shift+ArrowLeft');
  await expect(page.getByRole('tab', { name: 'hotsheet2' })).toHaveAttribute('aria-selected', 'true');
  await page.keyboard.press('ControlOrMeta+Alt+Shift+ArrowRight');
  await expect(page.getByRole('tab', { name: 'other' })).toHaveAttribute('aria-selected', 'true');

  // Settings uses a mnemonic chord instead of Safari's application-settings shortcut (⌘,).
  await page.keyboard.press('ControlOrMeta+Alt+S');
  await expect(page.getByRole('complementary', { name: 'Settings categories' })).toBeVisible();
  await page.getByRole('button', { name: 'Keyboard' }).click();
  const keyboardSettings = page.locator('[data-component="keyboard-settings"]');
  await expect(keyboardSettings.locator('li[data-shortcut-id="toggle-right-sidebar"]')).toContainText(
    /(Ctrl\+Alt\+Shift\+B|⌘⌥⇧B)/,
  );
  await expect(keyboardSettings.locator('li[data-shortcut-id="view-settings"]')).toContainText(/(Ctrl\+Alt\+S|⌘⌥S)/);
  await expect(keyboardSettings.locator('li[data-shortcut-id="project-tab-previous"]')).toContainText(
    /(Ctrl\+Alt\+Shift\+Left|⌘⌥⇧←)/,
  );
  await page.screenshot({ path: '/private/tmp/hs2-q1bh0v-browser-safe-shortcuts-wide.png', fullPage: true });
  await keyboardSettings
    .locator('li[data-shortcut-id="toggle-right-sidebar"]')
    .screenshot({ path: '/private/tmp/hs2-q1bh0v-right-sidebar-shortcut-after.png' });
  await keyboardSettings
    .locator('li[data-shortcut-id="view-settings"]')
    .screenshot({ path: '/private/tmp/hs2-q1bh0v-settings-shortcut-after.png' });
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
  await captureTabDefaults('/private/tmp/hs2-q1bh0v-navigation-shortcuts-after-wide.png');
  await page.setViewportSize({ width: 760, height: 700 });
  await captureTabDefaults('/private/tmp/hs2-q1bh0v-navigation-shortcuts-after-narrow.png');
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.locator('.view-mode-switcher [aria-label="List view"]').click();

  // Bare `c` opens the new-ticket composer; Escape closes it.
  await expect(composer).toBeHidden();
  await page.keyboard.press('c');
  await expect(composer).toBeVisible();
  await page.screenshot({ path: '/private/tmp/hs2-9shywd-shortcuts-wide.png', fullPage: true });
  await page.keyboard.press('Escape');
  await expect(composer).toBeHidden();

  // The bare `c` must NOT fire while a text field is focused: open search, type into it, and confirm
  // the composer stays closed (the character is typed, not consumed as a shortcut).
  await page.getByRole('button', { name: 'Search tickets' }).click();
  const search = page.locator('[data-token-search-editor="workspace-search"]');
  await expect(search).toBeFocused();
  await page.keyboard.press('c');
  await expect(composer).toBeHidden();
});
