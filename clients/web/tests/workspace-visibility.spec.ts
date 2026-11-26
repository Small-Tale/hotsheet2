import { expect, type Page, test } from '@playwright/test';

import { allFeaturesOn, featureSettingsView } from '../src/feature-areas';
import { openDemoProject, project } from './mobile-project-fixture';

/**
 * Manage Workspace Visibility through the shipped Workspace Grid toolbar, with mocked projects and
 * synthetic terminal names only. Captures land in `target/visual-captures/<ticket>/`.
 */
type FixtureTerminal = { id: string; alive: boolean; busy: boolean; cwd: string; kind?: 'ai' };

const DEMO_TERMINALS: FixtureTerminal[] = [
  { id: 'demo-shell', alive: true, busy: false, cwd: '/work/demo' },
  { id: 'demo-agent', alive: true, busy: false, cwd: '/work/demo', kind: 'ai' },
];

const TWO_PROJECT_TERMINALS: FixtureTerminal[] = [
  ...DEMO_TERMINALS,
  { id: 'demo-reviewer', alive: true, busy: false, cwd: '/work/demo', kind: 'ai' },
  { id: 'second-agent', alive: true, busy: false, cwd: '/work/second', kind: 'ai' },
];

/** Open `demo` and a second project, `second`, whose terminals the shared fixture owns by folder. */
async function openTwoProjects(page: Page) {
  await openDemoProject(page, true, 1, TWO_PROJECT_TERMINALS, undefined, { featureSettings: features(false) });
  const strip = page.locator('.project-tab-bar:not(.project-tab-bar--mobile)');
  await expect(strip.locator('[data-tab-kind="project"]')).toHaveCount(1);
  await page.route('**/__hotsheet/folders/choose', (route) => route.fulfill({ json: { path: '/work/second' } }));
  await page.route('**/__hotsheet/projects/open', (route) =>
    route.fulfill({
      status: 201,
      json: { ...project('second', '/work/second'), apiPath: '/__hotsheet/project-api/demo-checkout' },
    }),
  );
  await strip.getByRole('button', { name: 'Add Project' }).click();
  await page.getByRole('button', { name: 'Local Project' }).click();
  await expect(strip.locator('[data-tab-kind="project"]')).toHaveCount(2);
  return strip;
}

const features = (chat: boolean) => (chat ? allFeaturesOn() : featureSettingsView({ 'ai-chat': false }));

async function openGridVisibility(page: Page) {
  await page.getByRole('button', { name: 'Workspace Grid' }).click();
  await page.getByRole('button', { name: 'Manage workspace visibility' }).click();
  const dialog = page.locator('[data-terminal-visibility-dialog]');
  await expect(dialog).toHaveAttribute('open', '');
  return dialog;
}

const typeButton = (dialog: ReturnType<Page['locator']>, type: 'shell' | 'ai' | 'chat') =>
  dialog.locator(`[data-visibility-type="${type}"]`);

test('HS2-4TKASN hides AI Chats while the AI chat feature is off and no chat is open', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await openDemoProject(page, true, 1, DEMO_TERMINALS, undefined, { featureSettings: features(false) });
  const dialog = await openGridVisibility(page);
  const types = dialog.getByRole('group', { name: 'Item Types' }).getByRole('button');
  await expect(types).toHaveCount(2);
  await expect(typeButton(dialog, 'chat')).toHaveCount(0);
  await expect(dialog).not.toContainText('AI Chats');
  await expect(typeButton(dialog, 'shell')).toHaveAttribute('aria-pressed', 'true');
  await expect(typeButton(dialog, 'ai')).toHaveAttribute('aria-pressed', 'true');
  // The two segments share the row equally.
  const widths = await types.evaluateAll((buttons) => buttons.map((button) => button.getBoundingClientRect().width));
  expect(Math.abs(widths[0] - widths[1])).toBeLessThan(2);
  // Releasing both stores "nothing shown"; the grid follows, and both return.
  const rows = dialog.locator('[data-action="toggle-terminal-visibility"]');
  await expect(rows).toHaveCount(2);
  await typeButton(dialog, 'ai').click();
  await expect(rows).toHaveCount(1);
  await typeButton(dialog, 'shell').click();
  await expect(rows).toHaveCount(0);
  await expect(typeButton(dialog, 'shell')).toHaveAttribute('aria-pressed', 'false');
  await typeButton(dialog, 'shell').click();
  await typeButton(dialog, 'ai').click();
  await expect(rows).toHaveCount(2);
  await dialog.screenshot({
    path: 'target/visual-captures/hs2-4tkasn/chat-off-light-wide.png',
    animations: 'disabled',
  });
  await page.emulateMedia({ colorScheme: 'dark' });
  await dialog.screenshot({ path: 'target/visual-captures/hs2-4tkasn/chat-off-dark-wide.png', animations: 'disabled' });
  await page.setViewportSize({ width: 960, height: 800 });
  await dialog.screenshot({
    path: 'target/visual-captures/hs2-4tkasn/chat-off-dark-medium.png',
    animations: 'disabled',
  });
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(types).toHaveCount(2);
  await page.screenshot({ path: 'target/visual-captures/hs2-4tkasn/chat-off-dark-narrow.png', animations: 'disabled' });
  await page.emulateMedia({ colorScheme: 'light' });
  await page.screenshot({
    path: 'target/visual-captures/hs2-4tkasn/chat-off-light-narrow.png',
    animations: 'disabled',
  });
  await page.setViewportSize({ width: 1440, height: 900 });
  await expect(types).toHaveCount(2);
  await dialog.screenshot({
    path: 'target/visual-captures/hs2-4tkasn/chat-off-light-wide-return.png',
    animations: 'disabled',
  });
});

test('HS2-4TKASN shows AI Chats while the AI chat feature is on', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await openDemoProject(page, true, 1, DEMO_TERMINALS, undefined, { featureSettings: features(true) });
  const dialog = await openGridVisibility(page);
  await expect(dialog.getByRole('group', { name: 'Item Types' }).getByRole('button')).toHaveCount(3);
  await expect(typeButton(dialog, 'chat')).toHaveAccessibleName('AI Chats');
  await expect(typeButton(dialog, 'chat')).toHaveAttribute('aria-pressed', 'true');
  await dialog.screenshot({ path: 'target/visual-captures/hs2-4tkasn/chat-on-light-wide.png', animations: 'disabled' });
});

test('HS2-HC67SP HS2-DZH5GH toggles every listed item of one project from its name', async ({ page, browserName }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await openTwoProjects(page);
  const dialog = await openGridVisibility(page);
  const grid = page.getByRole('region', { name: 'Workspace Grid' });
  const tiles = grid.locator('[data-component="terminal-tile"]');
  await expect(tiles).toHaveCount(4);
  // Only AI shells are listed, so the heading names them.
  await typeButton(dialog, 'shell').click();
  const row = (name: string) => dialog.locator('[data-action="toggle-terminal-visibility"]', { hasText: name });
  await expect(dialog.locator('[data-action="toggle-terminal-visibility"]')).toHaveCount(3);
  const demoHeading = dialog.getByRole('button', { name: 'Hide All demo AI Shells' });
  await expect(demoHeading).toBeVisible();
  await expect(dialog.getByRole('button', { name: 'Hide All second AI Shells' })).toBeVisible();
  // HS2-DZH5GH: the project name itself is the button, inside the project heading, and the header has
  // no other control, so the heading is one tab stop that keeps its outline entry.
  const demoName = page.getByRole('button', { name: 'Hide All demo AI Shells' });
  const demoSection = dialog.locator('section').filter({ has: demoName });
  await expect(demoSection.getByRole('heading').filter({ has: demoName })).toHaveCount(1);
  await expect(demoHeading).toHaveText('demo');
  await expect(demoHeading).not.toHaveAttribute('aria-expanded', /.*/);
  await expect(demoSection.locator('[data-component="list-header"] button')).toHaveCount(1);
  await expect(demoSection.locator('[data-component="list-header"] svg')).toHaveCount(0);
  // WebKit on macOS moves Tab focus between buttons only with Option held.
  const tab = browserName === 'webkit' ? 'Alt+Tab' : 'Tab';
  await row('Demo Agent').focus();
  await page.keyboard.press(`Shift+${tab}`);
  await expect(demoHeading).toBeFocused();
  await dialog.screenshot({
    path: 'target/visual-captures/hs2-dzh5gh/name-focus-light-wide.png',
    animations: 'disabled',
  });
  await page.keyboard.press(tab);
  await expect(row('Demo Agent')).toBeFocused();
  // All visible: the heading hides all of demo's listed items, and only those.
  await demoHeading.click();
  await expect(row('Demo Agent')).toHaveAttribute('data-state', 'hidden');
  await expect(row('Demo Reviewer')).toHaveAttribute('data-state', 'hidden');
  await expect(row('Second Agent')).toHaveAttribute('data-state', 'visible');
  await expect(tiles).toHaveCount(1);
  // All hidden: the heading shows all.
  const showDemo = dialog.getByRole('button', { name: 'Show All demo AI Shells' });
  await showDemo.click();
  await expect(row('Demo Agent')).toHaveAttribute('data-state', 'visible');
  await expect(row('Demo Reviewer')).toHaveAttribute('data-state', 'visible');
  // Mixed: one hidden row makes the heading show all again. Keyboard activation reaches it.
  await row('Demo Reviewer').click();
  await expect(row('Demo Reviewer')).toHaveAttribute('data-state', 'hidden');
  await showDemo.focus();
  await page.keyboard.press('Enter');
  await expect(row('Demo Reviewer')).toHaveAttribute('data-state', 'visible');
  await expect(dialog.getByRole('button', { name: 'Hide All demo AI Shells' })).toBeFocused();
  await page.keyboard.press('Space');
  await expect(row('Demo Agent')).toHaveAttribute('data-state', 'hidden');
  // Terminals are not applied, so demo's shell is not in the grid until the segment returns.
  await expect(grid.locator('[data-terminal-key="demo-checkout:demo-shell"]')).toHaveCount(0);
  await typeButton(dialog, 'shell').click();
  await expect(dialog.getByRole('button', { name: 'Show All demo Items' })).toBeVisible();
  await expect(row('Demo Shell')).toHaveAttribute('data-state', 'visible');
  const capture = 'target/visual-captures/hs2-dzh5gh';
  await dialog.screenshot({ path: `${capture}/heading-light-wide.png`, animations: 'disabled' });
  await page.emulateMedia({ colorScheme: 'dark' });
  await dialog.screenshot({ path: `${capture}/heading-dark-wide.png`, animations: 'disabled' });
  await page.setViewportSize({ width: 960, height: 800 });
  await dialog.screenshot({ path: `${capture}/heading-dark-medium.png`, animations: 'disabled' });
  await page.setViewportSize({ width: 390, height: 844 });
  await page.screenshot({ path: `${capture}/heading-dark-narrow.png`, animations: 'disabled' });
  await page.emulateMedia({ colorScheme: 'light' });
  await page.screenshot({ path: `${capture}/heading-light-narrow.png`, animations: 'disabled' });
  await page.setViewportSize({ width: 1440, height: 900 });
  await dialog.screenshot({ path: `${capture}/heading-light-wide-return.png`, animations: 'disabled' });
});

test('HS2-W1XEH7 leaves minimized projects out of each workspace unless it shows them', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  const strip = await openTwoProjects(page);
  await strip.locator('[data-project-id="second"]').first().click({ button: 'right' });
  await page.getByRole('menuitem', { name: 'Minimize Tab' }).click();
  await expect(strip.locator('[data-tab-kind="project"]')).toHaveCount(1);
  const dialog = await openGridVisibility(page);
  const grid = page.getByRole('region', { name: 'Workspace Grid' });
  const tiles = grid.locator('[data-component="terminal-tile"]');
  const secondTile = grid.locator('[data-terminal-key="second:second-agent"]');
  const showMinimized = dialog.getByRole('checkbox', { name: 'Show minimized' });
  const secondHeading = dialog.getByRole('heading', { name: 'second' });
  // Off by default: the minimized project's items leave the dialog and the grid.
  await expect(showMinimized).not.toBeChecked();
  await expect(secondHeading).toHaveCount(0);
  await expect(dialog.locator('[data-action="toggle-terminal-visibility"]')).toHaveCount(3);
  await expect(tiles).toHaveCount(3);
  await expect(secondTile).toHaveCount(0);
  // Hide Listed acts only on listed items.
  await dialog.getByRole('button', { name: 'Hide Listed' }).click();
  await expect(tiles).toHaveCount(0);
  await dialog.getByRole('button', { name: 'Show Listed' }).click();
  await expect(tiles).toHaveCount(3);
  const capture = 'target/visual-captures/hs2-w1xeh7';
  await dialog.screenshot({ path: `${capture}/off-light-wide.png`, animations: 'disabled' });
  // Checked: they return, following their own visibility.
  await showMinimized.check();
  await expect(secondHeading).toBeVisible();
  await expect(dialog.locator('[data-action="toggle-terminal-visibility"]')).toHaveCount(4);
  await expect(secondTile).toHaveCount(1);
  await dialog.screenshot({ path: `${capture}/on-light-wide.png`, animations: 'disabled' });
  // A second workspace keeps its own choice, off by default.
  await dialog.getByRole('button', { name: 'Add Visibility Group' }).click();
  const nameDialog = page.locator('[data-terminal-visibility-name-dialog]');
  await nameDialog.getByRole('textbox', { name: 'Group Name' }).fill('AI');
  await nameDialog.getByRole('button', { name: 'Add' }).click();
  await expect(dialog.locator('[data-visibility-group-id]').filter({ hasText: 'AI' })).toHaveAttribute(
    'data-selected',
    'true',
  );
  await expect(showMinimized).not.toBeChecked();
  await expect(secondHeading).toHaveCount(0);
  // The grid follows the active workspace, which leaves the minimized project out.
  await expect(secondTile).toHaveCount(0);
  await dialog.locator('[data-visibility-group-id="default"]').click();
  await expect(showMinimized).toBeChecked();
  await expect(secondHeading).toBeVisible();
  // Keyboard: Space toggles it off and on again.
  await showMinimized.focus();
  await page.keyboard.press('Space');
  await expect(showMinimized).not.toBeChecked();
  await expect(secondTile).toHaveCount(0);
  await page.keyboard.press('Space');
  await expect(showMinimized).toBeChecked();
  await expect(secondTile).toHaveCount(1);
  await page.emulateMedia({ colorScheme: 'dark' });
  await dialog.screenshot({ path: `${capture}/on-dark-wide.png`, animations: 'disabled' });
  await page.setViewportSize({ width: 960, height: 800 });
  await dialog.screenshot({ path: `${capture}/on-dark-medium.png`, animations: 'disabled' });
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(showMinimized).toBeVisible();
  await page.screenshot({ path: `${capture}/on-dark-narrow.png`, animations: 'disabled' });
  await page.emulateMedia({ colorScheme: 'light' });
  await page.screenshot({ path: `${capture}/on-light-narrow.png`, animations: 'disabled' });
  await page.setViewportSize({ width: 1440, height: 900 });
  await dialog.screenshot({ path: `${capture}/on-light-wide-return.png`, animations: 'disabled' });
  // Reload: each workspace keeps its own choice.
  await page.reload();
  const stored = await page.evaluate(() => localStorage.getItem('hotsheet.terminals.visibility-groups'));
  const groups = (JSON.parse(stored ?? '{}') as { groups: Array<{ name: string; showMinimized?: boolean }> }).groups;
  expect(groups.map((group) => [group.name, Boolean(group.showMinimized)])).toEqual([
    ['Default', true],
    ['AI', false],
  ]);
});
