import { expect, test } from '@playwright/test';

import {
  captureWidths,
  createAndExpectStored,
  desktopApp,
  dialog,
  openNewTicket,
  responded,
  sessionSaved,
  stage,
} from './desktop-draft-files-fixture';

test('restores a parked draft while AI discovery is held on A→B→A (HS2-3VR31Y)', async ({ context, baseURL }) => {
  test.setTimeout(90_000);
  const app = await desktopApp(context, baseURL!);
  const aiReads: string[] = [];
  context.on('request', (request) => {
    const path = new URL(request.url()).pathname;
    if (
      path === `/__hotsheet/project-api/${app.server.checkoutId}/ai-tools` ||
      path === `/__hotsheet/project-api/${app.server.checkoutId}/ai-settings`
    )
      aiReads.push(path);
  });
  const held = app.pauseApi((method, path) => method === 'GET' && (path === '/ai-tools' || path === '/ai-settings'));
  try {
    const page = await app.launch();
    await openNewTicket(page, true);
    const title = dialog(page).getByRole('textbox', { name: 'Ticket Title' });
    await title.fill('Synthetic draft during discovery');
    const uploaded = responded(page, 'PUT');
    const persisted = sessionSaved(page, 'held-discovery.bin');
    await stage(page, [{ name: 'held-discovery.bin', bytes: [3, 1, 4] }]);
    await uploaded;
    await persisted;
    await expect.poll(() => aiReads.length).toBe(2);

    await app.addProject(page);
    await expect(dialog(page)).toBeHidden();
    const parked = JSON.parse(await app.storedSession()).composer;
    expect(parked.open).toBe(true);
    expect(parked.title).toBe('Synthetic draft during discovery');
    expect(parked.attachments.map((item: { name: string }) => item.name)).toEqual(['held-discovery.bin']);

    await page.getByRole('tab', { name: 'drafts', exact: true }).click();
    // The AI responses are still held. Restoration must depend only on the ticket/session reads.
    await expect(dialog(page)).toBeVisible();
    await expect(title).toHaveValue('Synthetic draft during discovery');
    await expect(dialog(page).getByText('held-discovery.bin', { exact: true })).toBeVisible();
    expect(aiReads).toHaveLength(2); // returning to A joins its pending ordinary read
    if (process.env.HOTSHEET_3VR_RESTORE_QA === '1') await captureWidths(page, '3vr31y-held-ai-restore');

    held.release();
    await createAndExpectStored(page, app, { 'held-discovery.bin': [3, 1, 4] });
  } finally {
    held.release();
    await app.stop();
  }
});
