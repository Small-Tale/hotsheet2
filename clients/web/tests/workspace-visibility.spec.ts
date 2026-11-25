import { expect, type Page, test } from '@playwright/test';

import { allFeaturesOn, featureSettingsView } from '../src/feature-areas';
import { openDemoProject } from './mobile-project-fixture';

/**
 * Manage Workspace Visibility through the shipped Workspace Grid toolbar, with mocked projects and
 * synthetic terminal names only. Captures land in `target/visual-captures/<ticket>/`.
 */
type FixtureTerminal = { id: string; alive: boolean; busy: boolean; cwd: string; kind?: 'ai' };

const DEMO_TERMINALS: FixtureTerminal[] = [
  { id: 'demo-shell', alive: true, busy: false, cwd: '/work/demo' },
  { id: 'demo-agent', alive: true, busy: false, cwd: '/work/demo', kind: 'ai' },
];

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
