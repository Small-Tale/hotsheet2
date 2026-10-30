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

interface RuleFilter {
  categories?: string[];
  priorities?: string[];
  views?: { checkout: string; view: string }[];
}

// HS2-DNQFB8: the rule dialog names open projects' saved views and filters by category and priority;
// editing a rule changes or clears them, and a view no open project offers stays listed by its id.
test('chooses saved views, categories, and priorities in the rule dialog', async ({ page }) => {
  const server = await realTicketServer();
  try {
    await page.setViewportSize({ width: 1280, height: 860 });
    await installTerminalFixture(page);
    await routeNotifierApi(page, server);
    await page.route(
      (url) => url.pathname.endsWith('/project-api/terminal-feedback/views'),
      (route) =>
        route.fulfill({
          json: [
            { id: 'release', name: 'Release', query: 'tag:release' },
            { id: 'needs-docs', name: 'Needs Docs', query: 'tag:docs AND is:open' },
          ],
        }),
    );
    await server.request('/notification-channels/desk-voice', 'PUT', {
      notifier: 'speak',
      label: 'Desk Voice',
      settings: { command: ['true'] },
    });
    await page.goto('/');
    await page.getByRole('button', { name: 'Open Project' }).click();
    await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();
    await page.getByLabel('Settings View').click();
    await page.getByRole('button', { name: 'Notifications', exact: true }).click();
    const settings = page.locator('[data-component="notification-settings"]');
    await expect(settings.getByText('Desk Voice').first()).toBeVisible();

    await settings.getByRole('button', { name: 'Add Rule…' }).click();
    const add = page.getByRole('dialog', { name: 'Add Rule' });
    await add.getByLabel('Name').fill('Release blockers');
    await add.getByRole('checkbox', { name: 'Blocked' }).check();
    await add.getByRole('checkbox', { name: 'Desk Voice' }).check();
    const views = add.getByRole('group', { name: 'In Saved Views' });
    await expect(views.getByRole('checkbox')).toHaveCount(2);
    await views.getByRole('checkbox', { name: 'Terminal feedback · Release' }).check();
    await add.getByLabel('Categories').fill('bug, feature');
    await add.getByLabel('Priorities').fill('high');
    for (const [label, width] of [
      ['wide', 1280],
      ['medium', 900],
      ['narrow', 420],
    ] as const) {
      await page.setViewportSize({ width, height: 860 });
      await settled(page);
      const overflow = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
      expect(overflow, `${label}: no horizontal page scroll`).toBeLessThanOrEqual(0);
      await page.screenshot({ path: `target/visual-captures/hs2-dnqfb8-rule-dialog-${label}.png` });
    }
    await page.setViewportSize({ width: 1280, height: 860 });
    await settled(page);
    await add.getByRole('button', { name: 'Add Rule' }).click();
    await expect(add).toBeHidden();
    const filter = async () =>
      (await server.request<{ rules: { id: string; filter?: RuleFilter }[] }>('/notification-config')).rules[0]?.filter;
    await expect.poll(filter).toEqual({
      categories: ['bug', 'feature'],
      priorities: ['high'],
      views: [{ checkout: 'terminal-feedback', view: 'release' }],
    });
    await expect(
      settings.getByText('To Desk Voice · category bug, feature · priority high · view terminal-feedback/release'),
    ).toBeVisible();

    // A view no open project offers (from the CLI, say) stays listed and checked by its id.
    await server.request('/notification-rules/release-blockers', 'PUT', {
      events: ['ticket.blocked'],
      channels: ['desk-voice'],
      filter: {
        categories: ['bug'],
        views: [
          { checkout: 'terminal-feedback', view: 'release' },
          { checkout: 'elsewhere-9z', view: 'ops' },
        ],
      },
    });
    // The category re-reads the server when it opens.
    await page.getByRole('button', { name: 'Keyboard', exact: true }).click();
    await page.getByRole('button', { name: 'Notifications', exact: true }).click();
    await expect(settings.getByText(/view terminal-feedback\/release, elsewhere-9z\/ops/)).toBeVisible();
    await rowMenu(page, 'Blocked', 'Edit…');
    const edit = page.getByRole('dialog', { name: 'Edit Rule' });
    const editViews = edit.getByRole('group', { name: 'In Saved Views' });
    await expect(editViews.getByRole('checkbox', { name: 'elsewhere-9z/ops' })).toBeChecked();
    await expect(editViews.getByRole('checkbox', { name: 'Terminal feedback · Release' })).toBeChecked();
    await expect(edit.getByLabel('Categories')).toHaveValue('bug');
    // Unchecking every view and clearing categories removes those filters.
    await editViews.getByRole('checkbox', { name: 'elsewhere-9z/ops' }).uncheck();
    await editViews.getByRole('checkbox', { name: 'Terminal feedback · Release' }).uncheck();
    await edit.getByLabel('Categories').fill('');
    await edit.getByLabel('Priorities').fill('highest');
    await edit.getByRole('button', { name: 'Save Rule' }).click();
    await expect(edit).toBeHidden();
    await expect.poll(filter).toEqual({ priorities: ['highest'] });
  } finally {
    await server.stop();
  }
});
