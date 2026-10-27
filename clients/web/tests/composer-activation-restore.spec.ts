import { expect, type Page, test } from '@playwright/test';

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

const commandsResponse = (page: Page, id: string) =>
  page.waitForResponse((response) => new URL(response.url()).pathname === `/__hotsheet/project-api/${id}/commands`);

test.describe('composer activation restoration (HS2-40JX5Q)', () => {
  test.setTimeout(90_000);

  for (const fails of [false, true])
    test(`preserves unsaved bytes and open intent through a persistence write while refresh ${fails ? 'fails, then activates again' : 'is held'}`, async ({
      context,
      baseURL,
    }) => {
      const app = await desktopApp(context, baseURL!);
      let refresh: ReturnType<typeof app.pauseApi> | undefined;
      try {
        const page = await app.launch();
        await openNewTicket(page, true);
        await dialog(page).getByRole('textbox', { name: 'Ticket Title' }).fill('Synthetic held unsaved draft');
        app.failWhen(({ method }) => method === 'PUT');
        const failed = responded(page, 'PUT'),
          persisted = sessionSaved(page, 'unsaved-held.bin');
        await stage(page, [{ name: 'unsaved-held.bin', bytes: [6, 7] }]);
        await failed;
        await persisted;
        await expect(dialog(page).getByText("Couldn't save", { exact: false })).toBeVisible();
        const parked = sessionSaved(page, 'unsaved-held.bin');
        await app.addProject(page);
        await parked;
        refresh = app.pauseApi((method, path) => method === 'GET' && path === '/commands');
        app.failApiWhen((method, path) => fails && method === 'GET' && path === '/commands');
        const placeholderSaved = sessionSaved(page, 'unsaved-held.bin');
        await page.getByRole('tab', { name: 'drafts', exact: true }).click();
        await refresh.started;
        await placeholderSaved;
        await expect(dialog(page)).toBeHidden();
        const stored = JSON.parse(await app.storedSession()).composer;
        expect(stored.open).toBe(true);
        expect(stored.title).toBe('Synthetic held unsaved draft');
        expect(stored.attachments.map((item: { name: string }) => item.name)).toEqual(['unsaved-held.bin']);
        const completed = commandsResponse(page, app.server.checkoutId);
        refresh.release();
        expect((await completed).status()).toBe(fails ? 500 : 200);
        await expect(dialog(page)).toBeVisible();
        await expect(dialog(page).getByRole('textbox', { name: 'Ticket Title' })).toHaveValue(
          'Synthetic held unsaved draft',
        );
        await expect(dialog(page).getByText('unsaved-held.bin', { exact: true })).toBeVisible();
        await expect(dialog(page).getByText("Couldn't save", { exact: false })).toBeVisible();
        await expect(dialog(page).getByRole('button', { name: 'Create ticket', exact: true })).toBeDisabled();
        if (fails) {
          // No new composer action: an actual failed refresh must leave this parked draft recoverable.
          app.failApiWhen(() => false);
          const parkedAgain = sessionSaved(page, 'unsaved-held.bin');
          await app.addProject(page);
          await parkedAgain;
          const durable = JSON.parse(await app.storedSession()).composer;
          expect(durable.open).toBe(true);
          expect(durable.title).toBe('Synthetic held unsaved draft');
          expect(durable.attachments.map((item: { name: string }) => item.name)).toEqual(['unsaved-held.bin']);
          await page.getByRole('tab', { name: 'drafts', exact: true }).click();
          await expect(dialog(page)).toBeVisible();
          await expect(dialog(page).getByRole('textbox', { name: 'Ticket Title' })).toHaveValue(
            'Synthetic held unsaved draft',
          );
          await expect(dialog(page).getByText('unsaved-held.bin', { exact: true })).toBeVisible();
          await expect(dialog(page).getByText("Couldn't save", { exact: false })).toBeVisible();
          await expect(dialog(page).getByRole('button', { name: 'Create ticket', exact: true })).toBeDisabled();
        }
        app.failWhen(() => false);
        const retried = responded(page, 'PUT');
        await dialog(page).getByRole('textbox', { name: 'Details' }).fill('Synthetic retry after delayed activation');
        await retried;
        await expect(dialog(page).getByRole('button', { name: 'Create ticket', exact: true })).toBeEnabled();
        await createAndExpectStored(page, app, { 'unsaved-held.bin': [6, 7] });
      } finally {
        refresh?.release();
        await app.stop();
      }
    });

  test('HS2-2QR67T keeps an immediate Details edit when deferred attachment focus completes after reset', async ({
    context,
    baseURL,
    browserName,
  }) => {
    const app = await desktopApp(context, baseURL!);
    let refresh: ReturnType<typeof app.pauseApi> | undefined;
    try {
      const page = await app.launch();
      await openNewTicket(page, true);
      await dialog(page).getByRole('textbox', { name: 'Ticket Title' }).fill('Synthetic parked original');
      await app.addProject(page);
      refresh = app.pauseApi((method, path) => method === 'GET' && path === '/commands');
      await page.getByRole('tab', { name: 'drafts', exact: true }).click();
      await refresh.started;
      await expect(dialog(page)).toBeHidden();
      await openNewTicket(page);
      await dialog(page).getByRole('button', { name: 'Cancel', exact: true }).click();
      await expect(dialog(page)).toBeHidden();
      await openNewTicket(page);
      const title = dialog(page).getByRole('textbox', { name: 'Ticket Title' }),
        details = dialog(page).getByRole('textbox', { name: 'Details' });
      await expect(title).toBeFocused();
      await title.fill('Synthetic newer replacement');
      // Hold actual frame callbacks while staging finishes. The next field's explicit focus and
      // real keyboard input must remain authoritative when those delayed callbacks resume.
      await page.evaluate(() => {
        const nativeFrame = window.requestAnimationFrame.bind(window),
          queued: FrameRequestCallback[] = [];
        window.requestAnimationFrame = (callback) => {
          queued.push(callback);
          document.body.dataset.hs2DeferredFrames = String(queued.length);
          return queued.length;
        };
        window.addEventListener(
          'hs2-release-composer-frames',
          () => {
            window.requestAnimationFrame = nativeFrame;
            for (const callback of queued) nativeFrame(callback);
          },
          { once: true },
        );
      });
      const saved = responded(page, 'PUT');
      await stage(page, [{ name: 'newer-replacement.bin', bytes: [4, 2] }]);
      await saved;
      await expect.poll(() => page.evaluate(() => Number(document.body.dataset.hs2DeferredFrames))).toBeGreaterThan(0);
      await details.focus();
      await page.keyboard.type('After the older refresh');
      await expect(details).toBeFocused();
      const finished = commandsResponse(page, app.server.checkoutId);
      refresh.release();
      await finished;
      await page.evaluate(async () => {
        window.dispatchEvent(new Event('hs2-release-composer-frames'));
        await new Promise<void>((resolve) =>
          requestAnimationFrame(() =>
            requestAnimationFrame(() => {
              resolve();
            }),
          ),
        );
      });
      expect(await page.evaluate(() => document.activeElement?.getAttribute('name'))).toBe('new-ticket-details');
      await expect(details).toBeFocused();
      await page.keyboard.type(' and the next edit');
      await expect(title).toHaveValue('Synthetic newer replacement');
      await expect(details).toHaveValue('After the older refresh and the next edit');
      await expect(dialog(page).getByText('newer-replacement.bin', { exact: true })).toBeVisible();
      if (process.env.HOTSHEET_2QR_FOCUS_QA === '1') await captureWidths(page, `2qr67t-focus-${browserName}`);
      await createAndExpectStored(page, app, { 'newer-replacement.bin': [4, 2] });
    } finally {
      refresh?.release();
      await app.stop();
    }
  });

  for (const outcome of ['switch', 'reset'] as const)
    for (const fails of [false, true])
      test(`keeps a newer draft after ${outcome} while an older refresh ${fails ? 'fails' : 'finishes'}`, async ({
        context,
        baseURL,
      }) => {
        const app = await desktopApp(context, baseURL!);
        let refresh: ReturnType<typeof app.pauseApi> | undefined;
        try {
          const page = await app.launch();
          await openNewTicket(page, true);
          await dialog(page).getByRole('textbox', { name: 'Ticket Title' }).fill('Synthetic parked original');
          const otherId = await app.addProject(page);
          const matches = (method: string, path: string) => method === 'GET' && path === '/commands';
          refresh = app.pauseApi(matches);
          app.failApiWhen((method, path) => fails && matches(method, path));
          await page.getByRole('tab', { name: 'drafts', exact: true }).click();
          await refresh.started;
          await expect(dialog(page)).toBeHidden();
          if (outcome === 'switch') {
            await page.getByRole('tab', { name: 'other-project', exact: true }).click();
          } else {
            await openNewTicket(page);
            await dialog(page).getByRole('button', { name: 'Cancel', exact: true }).click();
            await expect(dialog(page)).toBeHidden();
          }
          const persisted = sessionSaved(page, 'newer-replacement.bin');
          await openNewTicket(page);
          await dialog(page).getByRole('textbox', { name: 'Ticket Title' }).fill('Synthetic newer replacement');
          const saved = responded(page, 'PUT');
          await stage(page, [{ name: 'newer-replacement.bin', bytes: [4, 2] }]);
          await saved;
          await persisted;
          const finished = commandsResponse(page, app.server.checkoutId);
          refresh.release();
          await finished;
          const durable = JSON.parse(
            await app.storedSession(outcome === 'switch' ? otherId : app.server.checkoutId),
          ).composer;
          expect(durable.title).toBe('Synthetic newer replacement');
          expect(durable.attachments.map((item: { name: string }) => item.name)).toEqual(['newer-replacement.bin']);
          await expect(dialog(page).getByRole('textbox', { name: 'Ticket Title' })).toHaveValue(
            'Synthetic newer replacement',
          );
          await expect(dialog(page).getByText('newer-replacement.bin', { exact: true })).toBeVisible();
          await createAndExpectStored(page, app, { 'newer-replacement.bin': [4, 2] });
        } finally {
          refresh?.release();
          await app.stop();
        }
      });
});
