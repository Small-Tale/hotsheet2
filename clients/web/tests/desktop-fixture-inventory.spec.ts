import { expect, test } from '@playwright/test';

import { desktopApp, dialog, openNewTicket } from './desktop-draft-files-fixture';

test('HS2-T9TGKQ serves inventory without its stopped upstream before any page launch', async ({
  context,
  baseURL,
}) => {
  const app = await desktopApp(context, baseURL!);
  try {
    expect(context.pages()).toHaveLength(0);
    // No page, project open or discovery runs before the owned synthetic server exits.
    await app.server.stop();
    await expect(app.server.stop()).resolves.toBeUndefined();
    const prefix = `${app.origin}/__hotsheet/project-api/${app.server.checkoutId}`,
      tools = await context.request.get(`${prefix}/ai-tools`),
      settings = await context.request.get(`${prefix}/ai-settings`);
    expect(tools.status()).toBe(200);
    expect(await tools.json()).toEqual([]);
    expect(settings.status()).toBe(404);
    expect(await settings.json()).toEqual({ error: 'no drivable AI tools are installed' });
  } finally {
    // Final fixture cleanup re-stops the already exited upstream and closes its own listener.
    await expect(app.stop()).resolves.toBeUndefined();
  }
});

test('HS2-T9TGKQ uses an isolated empty AI inventory in the real desktop fixture', async ({ context, baseURL }) => {
  const app = await desktopApp(context, baseURL!);
  try {
    const page = await app.launch(),
      inventory = page.waitForResponse((response) => new URL(response.url()).pathname.endsWith('/ai-tools')),
      settings = page.waitForResponse((response) => new URL(response.url()).pathname.endsWith('/ai-settings'));
    await openNewTicket(page, true);
    const toolsResponse = await inventory,
      settingsResponse = await settings;
    expect(toolsResponse.status()).toBe(200);
    expect(await toolsResponse.json()).toEqual([]);
    expect(settingsResponse.status()).toBe(404);
    expect(await settingsResponse.json()).toEqual({ error: 'no drivable AI tools are installed' });
    await expect(dialog(page).getByRole('textbox', { name: 'Ticket Title' })).toBeVisible();
  } finally {
    await app.stop();
  }
});
