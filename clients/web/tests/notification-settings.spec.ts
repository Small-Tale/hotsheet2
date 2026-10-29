import { expect, type Page, test } from '@playwright/test';

import { realTicketServer } from './real-ticket-server';
import { installTerminalFixture } from './terminal-feedback-fixture';

interface Config {
  link_base?: string;
  channels: { id: string; notifier: string; label?: string; enabled: boolean; settings: Record<string, unknown> }[];
  rules: { id: string; events: string[]; channels: string[] }[];
}

const NOTIFIER_PATHS =
  /\/(notifiers|notification-config|notification-channels|notification-rules|notification-deliveries|notification-link-base|notification-project-timing)(\/|\?|$)/;

/** Forward only the notifier routes to the real server; the project itself stays a fixture. */
async function routeNotifierApi(page: Page, server: Awaited<ReturnType<typeof realTicketServer>>) {
  await page.route(
    (url) => NOTIFIER_PATHS.test(url.pathname),
    async (route) => {
      const url = new URL(route.request().url()),
        at = url.pathname.search(NOTIFIER_PATHS),
        path = `${url.pathname.slice(at)}${url.search}`;
      const response = await route.fetch({
        url: `${server.url}${path}`,
        headers: { ...route.request().headers(), 'X-Hotsheet-Secret': server.secret },
      });
      await route.fulfill({ response });
    },
  );
}

async function choose(page: Page, name: string, value: string) {
  const select = page.locator(`kui-select[name="${name}"]`);
  await select.locator('.kui-select__trigger').click();
  await expect(select).toHaveAttribute('open', '');
  await select.locator(`.kui-select__option[data-value="${value}"]`).click();
  await expect(select).toHaveJSProperty('value', value);
}

/** Let layout transitions and dialog motion finish so a capture shows the settled state. */
async function settled(page: Page) {
  await page.evaluate(async () => {
    const finite = document
      .getAnimations()
      .filter((animation) => animation.effect?.getComputedTiming().iterations !== Infinity);
    await Promise.all(finite.map((animation) => animation.finished.catch(() => undefined)));
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
  });
}

async function rowMenu(page: Page, label: string, item: string) {
  await page.getByRole('button', { name: `Actions for ${label}` }).click();
  await page.getByRole('menuitem', { name: item }).click();
}

// HS2-DPBJJA: App Settings → Notifications adds, tests, routes, disables, and removes a channel against
// the real server's notifier administration, at wide, medium, and narrow widths.
test('manages notifier channels and rules through App Settings → Notifications', async ({ page }) => {
  const server = await realTicketServer();
  try {
    await page.setViewportSize({ width: 1280, height: 860 });
    await installTerminalFixture(page);
    await routeNotifierApi(page, server);
    await page.goto('/');
    await page.getByRole('button', { name: 'Open Project' }).click();
    await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();
    await page.getByLabel('Settings View').click();
    await page.getByRole('button', { name: 'Notifications', exact: true }).click();
    const settings = page.locator('[data-component="notification-settings"]');
    await expect(settings.getByRole('heading', { name: 'Notifications' })).toBeVisible();
    await expect(settings.getByText('No channels yet.')).toBeVisible();
    await expect(settings.getByRole('button', { name: 'Add Rule…' })).toBeDisabled();

    // Add a speech channel whose command is `true`, so the test send succeeds offline.
    await settings.getByRole('button', { name: 'Add Channel…' }).click();
    const channelDialog = page.getByRole('dialog', { name: 'Add Channel' });
    await expect(channelDialog).toBeVisible();
    await expect(page.locator('kui-select[name="notification-channel-notifier"]')).toHaveJSProperty('value', 'speak');
    await channelDialog.getByLabel('Name').fill('Desk Voice');
    await channelDialog.getByLabel('Custom Command').fill('true');
    await channelDialog.getByRole('button', { name: 'Add Channel' }).click();
    await expect(channelDialog).toBeHidden();
    await expect
      .poll(async () => (await server.request<Config>('/notification-config')).channels)
      .toEqual([expect.objectContaining({ id: 'desk-voice', notifier: 'speak', label: 'Desk Voice', enabled: true })]);
    const saved = (await server.request<Config>('/notification-config')).channels[0];
    expect(saved.settings.command).toEqual(['true']);
    await expect(settings.getByText('Channel added.')).toBeVisible();

    // A credential field is a write-only password input that never shows a stored value.
    await settings.getByRole('button', { name: 'Add Channel…' }).click();
    await choose(page, 'notification-channel-notifier', 'ntfy');
    const token = page.getByRole('dialog', { name: 'Add Channel' }).getByLabel('Access Token');
    await expect(token).toHaveAttribute('type', 'password');
    await expect(token).toHaveValue('');
    await page.getByRole('dialog', { name: 'Add Channel' }).getByRole('button', { name: 'Cancel' }).click();
    await expect(page.getByRole('dialog', { name: 'Add Channel' })).toBeHidden();

    await rowMenu(page, 'Desk Voice', 'Send Test');
    await expect(settings.getByText('Test notification sent.')).toBeVisible();
    await expect(settings.getByText(/Delivered to desk-voice/)).toBeVisible();

    // A rule that sends project.stopped to the new channel.
    await settings.getByRole('button', { name: 'Add Rule…' }).click();
    const ruleDialog = page.getByRole('dialog', { name: 'Add Rule' });
    await ruleDialog.getByLabel('Name').fill('Stopped');
    await ruleDialog.getByRole('checkbox', { name: 'Project Stopped or Stuck' }).check();
    await ruleDialog.getByRole('checkbox', { name: 'Desk Voice' }).check();
    await ruleDialog.getByRole('button', { name: 'Add Rule' }).click();
    await expect(ruleDialog).toBeHidden();
    await expect
      .poll(async () => (await server.request<Config>('/notification-config')).rules)
      .toEqual([expect.objectContaining({ id: 'stopped', events: ['project.stopped'], channels: ['desk-voice'] })]);
    await expect(settings.getByText('To Desk Voice')).toBeVisible();

    // The link origin saves on change.
    const origin = settings.getByLabel('Link Origin');
    await origin.fill('https://hotsheet.example.net');
    await origin.press('Tab');
    await expect
      .poll(async () => (await server.request<Config>('/notification-config')).link_base)
      .toBe('https://hotsheet.example.net');

    await rowMenu(page, 'Desk Voice', 'Disable');
    await expect
      .poll(async () => (await server.request<Config>('/notification-config')).channels[0]?.enabled)
      .toBe(false);
    await expect(settings.getByText('Disabled', { exact: true })).toBeVisible();

    for (const [label, width] of [
      ['wide', 1280],
      ['medium', 900],
      ['narrow', 420],
    ] as const) {
      await page.setViewportSize({ width, height: 860 });
      await expect(settings).toBeVisible();
      await settled(page);
      const overflow = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
      expect(overflow, `${label}: no horizontal page scroll`).toBeLessThanOrEqual(0);
      await page.screenshot({ path: `target/visual-captures/hs2-dpbjja-notifications-${label}.png`, fullPage: true });
      await settings.getByRole('heading', { name: 'Recent Deliveries' }).scrollIntoViewIfNeeded();
      await settled(page);
      await page.screenshot({ path: `target/visual-captures/hs2-dpbjja-notifications-${label}-lower.png` });
      await settings.getByRole('heading', { name: 'Notifications' }).scrollIntoViewIfNeeded();
    }
    await page.setViewportSize({ width: 1280, height: 860 });
    await settings.getByRole('button', { name: 'Add Channel…' }).click();
    await settled(page);
    await page.screenshot({ path: 'target/visual-captures/hs2-dpbjja-channel-dialog-wide.png' });
    await page.setViewportSize({ width: 420, height: 860 });
    await settled(page);
    await page.screenshot({ path: 'target/visual-captures/hs2-dpbjja-channel-dialog-narrow.png' });
    await page.getByRole('dialog', { name: 'Add Channel' }).getByRole('button', { name: 'Cancel' }).click();
    await page.setViewportSize({ width: 1280, height: 860 });

    // Removing the channel also removes the rule that used only it.
    await rowMenu(page, 'Desk Voice', 'Remove');
    await expect
      .poll(async () => await server.request<Config>('/notification-config'))
      .toMatchObject({
        channels: [],
        rules: [],
      });
    await expect(settings.getByText('Channel removed with 1 rule that used only it.')).toBeVisible();
    await expect(settings.getByText('No channels yet.')).toBeVisible();
  } finally {
    await server.stop();
  }
});
