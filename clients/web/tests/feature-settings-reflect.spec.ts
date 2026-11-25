import { execFileSync } from 'node:child_process';

import { expect, type Page, test } from '@playwright/test';

import { openMockedProject } from './open-mocked-project';
import { proxyFetch, realTicketServer } from './real-ticket-server';

/**
 * HS2-N3QBVG: App Settings › Features shows the machine's saved choices, including after the page
 * reloads (a rebuild) and when the settings read answers only after the page has rendered.
 */
type Server = Awaited<ReturnType<typeof realTicketServer>>;

async function open(page: Page, server: Server, hold?: Promise<void>) {
  const apiPath = `/__hotsheet/project-api/${server.checkoutId}`;
  await page.routeWebSocket(/.*/, (route) => {
    void route.close();
  });
  await page.route(/\/__hotsheet\/(?:remote\/)?projects\/open$/, (route) =>
    route.fulfill({
      status: 201,
      json: { id: server.checkoutId, root: server.root, name: 'Launchpad', stores: [server.store], apiPath },
    }),
  );
  await page.route(`**${apiPath}/**`, async (route) => {
    const incoming = new URL(route.request().url()),
      path = incoming.pathname.slice(apiPath.length);
    if (hold && path === '/feature-settings' && route.request().method() === 'GET') await hold;
    try {
      const response = await proxyFetch(route, {
        url: `${server.url}${path}${incoming.search}`,
        headers: { ...route.request().headers(), 'X-Hotsheet-Secret': server.secret },
        timeout: 60_000,
      });
      await route.fulfill({ response });
    } catch {
      await route.abort().catch(() => undefined);
    }
  });
}

async function showFeatures(page: Page) {
  await page.getByLabel('Settings View').click();
  await page.getByRole('navigation', { name: 'App Settings' }).getByRole('button', { name: 'Features' }).click();
  return page.locator('[data-settings-category="features"]');
}

test('HS2-N3QBVG Features shows saved choices on open, after a reload, and after a late read', async ({ page }) => {
  test.setTimeout(90_000);
  await page.setViewportSize({ width: 1440, height: 1100 });
  const server = await realTicketServer({ features: 'defaults' });
  try {
    // Choices saved on the machine before this client opens (as after a rebuild).
    for (const setting of ['features.drive', 'features.ai-chat'])
      execFileSync(server.cli, ['settings', 'set', setting, 'on'], { env: server.env });
    let release!: () => void;
    const hold = new Promise<void>((resolve) => {
      release = resolve;
    });
    await open(page, server, hold);
    console.log('STEP', "await page.goto('/');");
    await page.goto('/');
    await openMockedProject(page);
    const features = await showFeatures(page);
    const drive = features.getByRole('switch', { name: 'Drive with AI' }),
      chat = features.locator('[data-feature-row="ai-chat"] input[role="switch"]');
    await expect(drive).toBeVisible();
    // The read answers after the page rendered with the registered defaults.
    release();
    await expect(drive).toBeChecked();
    await expect(chat).toBeChecked();
    // A reload while on the Features page (a rebuild reloads the client).
    await page.reload();
    const again = await showFeatures(page);
    await expect(again.getByRole('switch', { name: 'Drive with AI' })).toBeChecked();
    await expect(again.locator('[data-feature-row="ai-chat"] input[role="switch"]')).toBeChecked();
  } finally {
    await page.unrouteAll({ behavior: 'wait' });
    await server.stop();
  }
});
