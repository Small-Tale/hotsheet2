import { expect, test } from '@playwright/test';

import {
  createAndExpectStored,
  desktopApp,
  dialog,
  openNewTicket,
  responded,
  sessionSaved,
  stage,
} from './desktop-draft-files-fixture';

for (const relaunch of [false, true])
  test(`HS2-ZMVQR4 preserves real attachment bytes ${relaunch ? 'after desktop recovery relaunch' : 'on direct Create'}`, async ({
    context,
    baseURL,
  }) => {
    const app = await desktopApp(context, baseURL!);
    try {
      let page = await app.launch();
      await openNewTicket(page, true);
      await dialog(page).getByRole('textbox', { name: 'Ticket Title' }).fill('Synthetic byte transport proof');
      const saved = responded(page, 'PUT', 2),
        persisted = sessionSaved(page, 'unicode-界.bin');
      await stage(page, [
        { name: 'binary-proof.bin', bytes: [0, 255, 128, 42, 10] },
        { name: 'unicode-界.bin', bytes: [4, 2] },
      ]);
      await saved;
      await persisted;
      expect(await app.records()).toHaveLength(2);
      if (relaunch) {
        page = await app.launch(page);
        await openNewTicket(page);
        await expect(dialog(page).getByRole('textbox', { name: 'Ticket Title' })).toHaveValue(
          'Synthetic byte transport proof',
        );
      }
      await expect(dialog(page).getByText('binary-proof.bin', { exact: true })).toBeVisible();
      await expect(dialog(page).getByText('unicode-界.bin', { exact: true })).toBeVisible();
      await createAndExpectStored(page, app, {
        'binary-proof.bin': [0, 255, 128, 42, 10],
        'unicode-界.bin': [4, 2],
      });
    } finally {
      await app.stop();
    }
  });
