import { readFile } from 'node:fs/promises';

import { expect, test } from '@playwright/test';

import {
  captureWidths,
  createAndExpectStored,
  desktopApp,
  dialog,
  findFile,
  openNewTicket,
  openNotWorking,
  responded,
  sessionSaved,
  stage,
} from './desktop-draft-files-fixture';

test.describe('desktop draft attachment persistence (HS2-791W19)', () => {
  test.setTimeout(150_000);

  test('restores staged ticket attachments from desktop app data after a relaunch on another port', async ({
    context,
    baseURL,
  }) => {
    const app = await desktopApp(context, baseURL!);
    try {
      let page = await app.launch();
      await openNewTicket(page, true);
      await dialog(page).getByRole('textbox', { name: 'Ticket Title' }).fill('Draft that survives a relaunch');
      const saved = responded(page, 'PUT', 2),
        persisted = sessionSaved(page, 'discarded-proof.txt');
      await stage(page, [
        { name: 'kept-proof.bin', bytes: [0, 255, 1, 128, 10] },
        { name: 'discarded-proof.txt', bytes: [115, 121, 110] },
      ]);
      await saved;
      await persisted;
      expect(await app.records()).toHaveLength(2);

      page = await app.launch(page);
      await openNewTicket(page);
      await expect(dialog(page).getByRole('textbox', { name: 'Ticket Title' })).toHaveValue(
        'Draft that survives a relaunch',
      );
      await expect(dialog(page).getByText('kept-proof.bin')).toBeVisible();
      await expect(dialog(page).getByText('discarded-proof.txt')).toBeVisible();
      await captureWidths(page, 'restored-desktop-draft');

      const removed = responded(page, 'DELETE');
      await dialog(page).getByRole('button', { name: 'Remove discarded-proof.txt' }).click();
      await removed;
      expect(await app.records()).toHaveLength(1);
      await createAndExpectStored(page, app, { 'kept-proof.bin': [0, 255, 1, 128, 10] });
      expect(await findFile(app.server.store, 'discarded-proof.txt')).toBeUndefined();
    } finally {
      await app.stop();
    }
  });

  for (const status of [404, 500] as const)
    test(`keeps a staged file whose recovery copy returned ${String(status)}, then restores it on the next relaunch`, async ({
      context,
      baseURL,
    }) => {
      const app = await desktopApp(context, baseURL!);
      try {
        let page = await app.launch();
        await openNewTicket(page, true);
        await dialog(page).getByRole('textbox', { name: 'Ticket Title' }).fill('Draft with one unreadable copy');
        const saved = responded(page, 'PUT', 2),
          persisted = sessionSaved(page, 'second-proof.bin');
        await stage(page, [
          { name: 'first-proof.bin', bytes: [1, 2, 3] },
          { name: 'second-proof.bin', bytes: [4, 5, 6] },
        ]);
        await saved;
        await persisted;

        // One of the two reads fails on this launch.
        const secondId = JSON.parse(await app.storedSession()).composer.attachments.find(
          (item: { name: string }) => item.name === 'second-proof.bin',
        ).id as string;
        app.failWhen(({ method, key }) =>
          method === 'GET' && key.endsWith(`:${secondId}`) ? (status === 404 ? 404 : true) : false,
        );
        page = await app.launch(page);
        await openNewTicket(page);
        await expect(dialog(page).getByText('first-proof.bin')).toBeVisible();
        await expect(dialog(page).getByText('second-proof.bin', { exact: true })).toBeVisible();
        const warning = dialog(page).getByText("Couldn't restore 1 staged file.", { exact: false });
        await expect(warning).toBeVisible();
        await expect(dialog(page).getByRole('button', { name: 'Create ticket', exact: true })).toBeDisabled();
        // Programmatic form submit must enforce the same guard as the disabled button.
        await dialog(page)
          .locator('form')
          .evaluate((form: HTMLFormElement) => {
            form.requestSubmit();
          });
        await expect(dialog(page)).toBeVisible();
        expect(await app.records()).toHaveLength(2);
        const healthySave = responded(page, 'PUT');
        await stage(page, [{ name: 'healthy-proof.bin', bytes: [7, 8] }]);
        await healthySave;
        await expect(warning).toBeVisible();
        const healthyDelete = responded(page, 'DELETE');
        await dialog(page).getByRole('button', { name: 'Remove healthy-proof.bin' }).click();
        await healthyDelete;
        await expect(warning).toBeVisible();
        await expect(dialog(page).getByRole('button', { name: 'Create ticket', exact: true })).toBeDisabled();
        await captureWidths(page, 'unrestored-desktop-draft');

        // A later session save keeps the unreadable reference instead of dropping it.
        const kept = sessionSaved(page, 'Draft with one unreadable copy, edited');
        await dialog(page)
          .getByRole('textbox', { name: 'Ticket Title' })
          .fill('Draft with one unreadable copy, edited');
        await kept;
        expect(
          JSON.parse(await app.storedSession())
            .composer.attachments.map((item: { name: string }) => item.name)
            .sort(),
        ).toEqual(['first-proof.bin', 'second-proof.bin']);

        app.failWhen(() => false);
        page = await app.launch(page);
        await openNewTicket(page);
        await expect(dialog(page).getByText('first-proof.bin')).toBeVisible();
        await expect(dialog(page).getByText('second-proof.bin')).toBeVisible();
        await expect(dialog(page).getByText("Couldn't restore", { exact: false })).toHaveCount(0);
        await captureWidths(page, 'recovered-desktop-draft');
        await createAndExpectStored(page, app, { 'first-proof.bin': [1, 2, 3], 'second-proof.bin': [4, 5, 6] });
      } finally {
        await app.stop();
      }
    });

  test('keeps a staged file whose recovery copy failed to save, retries it, and restores it after relaunch', async ({
    context,
    baseURL,
  }) => {
    const app = await desktopApp(context, baseURL!);
    try {
      let page = await app.launch();
      await openNewTicket(page, true);
      await dialog(page).getByRole('textbox', { name: 'Ticket Title' }).fill('Draft saved on retry');
      // The bridge reports a full disk until this test lets saves through again.
      let diskFull = true;
      app.failWhen(({ method }) => method === 'PUT' && diskFull);
      const failed = responded(page, 'PUT');
      await stage(page, [{ name: 'retried-proof.bin', bytes: [9, 8, 7, 0] }]);
      expect((await failed).status()).toBe(500);
      // The file stays attached with a recoverable warning; nothing was recorded yet.
      await expect(dialog(page).getByText('retried-proof.bin', { exact: true })).toBeVisible();
      const warning = dialog(page).getByText("Couldn't save “retried-proof.bin” for recovery", { exact: false });
      await expect(warning).toBeVisible();
      await expect(dialog(page).getByRole('button', { name: 'Create ticket', exact: true })).toBeDisabled();
      await dialog(page)
        .locator('form')
        .evaluate((form: HTMLFormElement) => {
          form.requestSubmit();
        });
      await expect(dialog(page)).toBeVisible();
      expect(await app.records()).toEqual([]);
      // Resizing persists the session too; those retries also fail and keep the warning.
      await captureWidths(page, 'unsaved-desktop-draft');
      await expect(warning).toBeVisible();
      diskFull = false;

      // The next change persists the session, which retries the save and clears the warning.
      const retried = responded(page, 'PUT'),
        persisted = sessionSaved(page, 'One more detail');
      await dialog(page).getByRole('textbox', { name: 'Details' }).fill('One more detail');
      expect((await retried).status()).toBe(204);
      await persisted;
      await expect(warning).toBeHidden();
      expect(await app.records()).toHaveLength(1);

      page = await app.launch(page);
      await openNewTicket(page);
      await expect(dialog(page).getByText('retried-proof.bin', { exact: true })).toBeVisible();
      await createAndExpectStored(page, app, { 'retried-proof.bin': [9, 8, 7, 0] });
    } finally {
      await app.stop();
    }
  });
  for (const fails of [false, true])
    test(`HS2-791W19 keeps a later project draft when an earlier Create ${fails ? 'fails' : 'finishes'}`, async ({
      context,
      baseURL,
    }) => {
      const app = await desktopApp(context, baseURL!);
      const hold = app.pauseApi((method, path) => method === 'POST' && path.endsWith('/tickets'));
      app.failApiWhen((method, path) => fails && method === 'POST' && path.endsWith('/tickets'));
      try {
        const page = await app.launch();
        await openNewTicket(page, true);
        await dialog(page).getByRole('textbox', { name: 'Ticket Title' }).fill('Synthetic pending create');
        const saved = responded(page, 'PUT');
        await stage(page, [{ name: 'origin-proof.bin', bytes: [1, 2] }]);
        await saved;
        await expect(dialog(page).getByRole('button', { name: 'Create ticket', exact: true })).toBeEnabled();
        await dialog(page).getByRole('button', { name: 'Create ticket', exact: true }).click();
        await hold.started;
        const completed = page.waitForEvent('requestfinished', {
          predicate: (request) => request.method() === 'POST' && new URL(request.url()).pathname.endsWith('/tickets'),
        });
        // A modal creation owns the foreground; switch by the real project tab controls after dismissing only its surface.
        await page.keyboard.press('Escape');
        const otherId = await app.addProject(page);
        await openNewTicket(page);
        await dialog(page).getByRole('textbox', { name: 'Ticket Title' }).fill('Synthetic later draft');
        const laterSaved = responded(page, 'PUT'),
          laterPersisted = sessionSaved(page, 'later-proof.bin');
        await stage(page, [{ name: 'later-proof.bin', bytes: [3, 4] }]);
        await laterSaved;
        await laterPersisted;
        hold.release();
        await completed;
        await page.evaluate(
          () =>
            new Promise<void>((resolve) =>
              requestAnimationFrame(() => {
                resolve();
              }),
            ),
        );
        if (!fails) await expect.poll(() => findFile(app.server.store, 'origin-proof.bin')).toBeDefined();
        else expect(JSON.parse(await app.storedSession()).composer.attachments).toHaveLength(1);
        await expect(dialog(page).getByText('Synthetic operation rejected.', { exact: false })).toHaveCount(0);
        await expect(dialog(page).getByRole('textbox', { name: 'Ticket Title' })).toHaveValue('Synthetic later draft');
        await expect(dialog(page).getByText('later-proof.bin', { exact: true })).toBeVisible();
        expect(JSON.parse(await app.storedSession(otherId)).composer.attachments).toHaveLength(1);
      } finally {
        hold.release();
        await app.stop();
      }
    });
  test('HS2-791W19 blocks Not working on an unrestored file until explicit removal', async ({ context, baseURL }) => {
    const app = await desktopApp(context, baseURL!);
    try {
      const ticket = await app.completedTicket();
      let page = await app.launch();
      await openNewTicket(page, true);
      await dialog(page).getByRole('button', { name: 'Cancel', exact: true }).click();
      let report = await openNotWorking(page, ticket);
      await report.getByRole('textbox', { name: 'What’s wrong?' }).fill('Synthetic regression report');
      const saved = responded(page, 'PUT', 2),
        persisted = sessionSaved(page, 'missing-report.bin');
      await report.getByLabel('Browse evidence attachments').setInputFiles([
        { name: 'kept-report.bin', mimeType: 'application/octet-stream', buffer: Buffer.from([1, 3, 5]) },
        { name: 'missing-report.bin', mimeType: 'application/octet-stream', buffer: Buffer.from([2, 4, 6]) },
      ]);
      await saved;
      await persisted;
      const missingId = JSON.parse(await app.storedSession()).notWorking.attachments.find(
        (item: { name: string }) => item.name === 'missing-report.bin',
      ).id as string;
      app.failWhen(({ method, key }) => method === 'GET' && key.endsWith(`:${missingId}`));
      page = await app.launch(page);
      report = page.getByRole('dialog', { name: `Not Working — ${ticket.slug}` });
      await expect(report.getByText('missing-report.bin', { exact: true })).toBeVisible();
      await expect(report.getByRole('button', { name: 'Report Not Working', exact: true })).toBeDisabled();
      await report.locator('form').evaluate((form: HTMLFormElement) => {
        form.requestSubmit();
      });
      await expect(report.getByRole('textbox')).toBeVisible();
      expect(await app.records()).toHaveLength(2);
      await captureWidths(page, 'not-working-failed-recovery', report.locator('dialog'));
      const removed = responded(page, 'DELETE');
      await report.getByRole('button', { name: 'Remove missing-report.bin' }).click();
      await removed;
      await expect(report.getByText("Couldn't restore", { exact: false })).toHaveCount(0);
      await expect(report.getByRole('button', { name: 'Report Not Working', exact: true })).toBeEnabled();
      await captureWidths(page, 'not-working-explicit-removal', report.locator('dialog'));
      await report.getByRole('button', { name: 'Report Not Working', exact: true }).click();
      await expect(report).toBeHidden();
      await expect.poll(() => findFile(app.server.store, 'kept-report.bin')).toBeDefined();
      expect([...(await readFile((await findFile(app.server.store, 'kept-report.bin'))!))]).toEqual([1, 3, 5]);
      await expect.poll(() => app.records()).toEqual([]);
    } finally {
      await app.stop();
    }
  });

  test('HS2-791W19 keeps an unsaved file and warning after switching away and back', async ({ context, baseURL }) => {
    const app = await desktopApp(context, baseURL!);
    try {
      const page = await app.launch();
      await openNewTicket(page, true);
      await dialog(page).getByRole('textbox', { name: 'Ticket Title' }).fill('Synthetic unsaved origin');
      app.failWhen(({ method }) => method === 'PUT');
      const failed = responded(page, 'PUT');
      await stage(page, [{ name: 'unsaved-switch.bin', bytes: [6, 7] }]);
      await failed;
      await expect(dialog(page).getByText("Couldn't save", { exact: false })).toBeVisible();
      await app.addProject(page);
      await page.getByRole('tab', { name: 'drafts', exact: true }).click();
      await openNewTicket(page);
      await expect(dialog(page).getByText('unsaved-switch.bin', { exact: true })).toBeVisible();
      await expect(dialog(page).getByText("Couldn't save", { exact: false })).toBeVisible();
      await expect(dialog(page).getByRole('button', { name: 'Create ticket', exact: true })).toBeDisabled();
      app.failWhen(() => false);
      const retried = responded(page, 'PUT');
      await dialog(page).getByRole('textbox', { name: 'Details' }).fill('Synthetic retry edit');
      await retried;
      await expect(dialog(page).getByRole('button', { name: 'Create ticket', exact: true })).toBeEnabled();
      await createAndExpectStored(page, app, { 'unsaved-switch.bin': [6, 7] });
    } finally {
      await app.stop();
    }
  });

  test('HS2-791W19 ignores a late staging response after cancel and reopen', async ({ context, baseURL }) => {
    const app = await desktopApp(context, baseURL!);
    let key = '';
    const hold = app.pauseBridge((method, candidate) => {
      if (method !== 'PUT') return false;
      key ||= candidate;
      return key === candidate;
    });
    try {
      const page = await app.launch();
      await openNewTicket(page, true);
      await stage(page, [{ name: 'cancelled.bin', bytes: [5, 5] }]);
      await hold.started;
      await dialog(page).getByRole('button', { name: 'Cancel', exact: true }).click();
      await openNewTicket(page);
      await dialog(page).getByRole('textbox', { name: 'Ticket Title' }).fill('Synthetic replacement');
      const saved = responded(page, 'PUT');
      await stage(page, [{ name: 'replacement.bin', bytes: [8, 9] }]);
      await saved;
      hold.release();
      await expect(dialog(page).getByRole('textbox', { name: 'Ticket Title' })).toHaveValue('Synthetic replacement');
      await expect(dialog(page).getByText('cancelled.bin', { exact: true })).toHaveCount(0);
      await expect(dialog(page).getByText("Couldn't", { exact: false })).toHaveCount(0);
      await expect(dialog(page).getByRole('button', { name: 'Create ticket', exact: true })).toBeEnabled();
      await createAndExpectStored(page, app, { 'replacement.bin': [8, 9] });
    } finally {
      hold.release();
      await app.stop();
    }
  });

  for (const fails of [false, true])
    test(`HS2-791W19 keeps a newer Not working target when an earlier report ${fails ? 'fails' : 'finishes'}`, async ({
      context,
      baseURL,
    }) => {
      const app = await desktopApp(context, baseURL!);
      const matches = (method: string, path: string) => method === 'POST' && path.endsWith('/not-working');
      const hold = app.pauseApi(matches);
      app.failApiWhen((method, path) => fails && matches(method, path));
      try {
        const oldTicket = await app.completedTicket('Synthetic first completed ticket'),
          laterTicket = await app.completedTicket('Synthetic second completed ticket');
        const page = await app.launch();
        await openNewTicket(page, true);
        await dialog(page).getByRole('button', { name: 'Cancel', exact: true }).click();
        const oldReport = await openNotWorking(page, oldTicket);
        await oldReport.getByRole('textbox').fill('Synthetic first report');
        const saved = responded(page, 'PUT');
        await oldReport.getByLabel('Browse evidence attachments').setInputFiles({
          name: 'submitted-report.bin',
          mimeType: 'application/octet-stream',
          buffer: Buffer.from([4, 7]),
        });
        await saved;
        await oldReport.getByRole('button', { name: 'Report Not Working', exact: true }).click();
        await hold.started;
        const completed = page.waitForEvent('requestfinished', {
          predicate: (request) => matches(request.method(), new URL(request.url()).pathname),
        });
        const otherId = await app.addProject(page);
        const later = await openNotWorking(page, laterTicket);
        await later.getByRole('textbox').fill('Synthetic later report');
        const laterSaved = responded(page, 'PUT'),
          persisted = sessionSaved(page, 'later-report.bin');
        await later.getByLabel('Browse evidence attachments').setInputFiles({
          name: 'later-report.bin',
          mimeType: 'application/octet-stream',
          buffer: Buffer.from([8, 3]),
        });
        await laterSaved;
        await persisted;
        if (fails) {
          await later.getByRole('button', { name: 'Cancel', exact: true }).click();
          await openNotWorking(page, laterTicket);
          await later.getByRole('textbox').fill('Synthetic later report');
          const savedAgain = responded(page, 'PUT'),
            persistedAgain = sessionSaved(page, 'later-report.bin');
          await later.getByLabel('Browse evidence attachments').setInputFiles({
            name: 'later-report.bin',
            mimeType: 'application/octet-stream',
            buffer: Buffer.from([8, 3]),
          });
          await savedAgain;
          await persistedAgain;
        }
        hold.release();
        await completed;
        await page.evaluate(
          () =>
            new Promise<void>((resolve) =>
              requestAnimationFrame(() => {
                resolve();
              }),
            ),
        );
        if (!fails) await expect.poll(() => findFile(app.server.store, 'submitted-report.bin')).toBeDefined();
        else expect(JSON.parse(await app.storedSession()).notWorking.attachments).toHaveLength(1);
        await expect(later.getByRole('textbox')).toHaveValue('Synthetic later report');
        await expect(later.getByText('later-report.bin', { exact: true })).toBeVisible();
        await expect(later.getByText('Synthetic operation rejected.', { exact: false })).toHaveCount(0);
        expect(JSON.parse(await app.storedSession(otherId)).notWorking.attachments).toHaveLength(1);
        // A further same-project cancel/reopen target survives the old callback too.
        await later.getByRole('button', { name: 'Cancel', exact: true }).click();
        await expect(later.getByRole('textbox')).toBeHidden();
      } finally {
        hold.release();
        await app.stop();
      }
    });

  test('HS2-791W19 ignores held restore GET after same-project cancel, reopen and new staging', async ({
    context,
    baseURL,
  }) => {
    const app = await desktopApp(context, baseURL!);
    let hold: ReturnType<typeof app.pauseBridge> | undefined;
    try {
      const page = await app.launch();
      await openNewTicket(page, true);
      await dialog(page).getByRole('textbox', { name: 'Ticket Title' }).fill('Synthetic saved original');
      const saved = responded(page, 'PUT'),
        persisted = sessionSaved(page, 'restore-old.bin');
      await stage(page, [{ name: 'restore-old.bin', bytes: [1, 7] }]);
      await saved;
      await persisted;
      await app.addProject(page);
      hold = app.pauseBridge((method, key) => method === 'GET' && key.startsWith(`composer:${app.server.checkoutId}:`));
      await page.getByRole('tab', { name: 'drafts', exact: true }).click();
      await hold.started;
      await openNewTicket(page);
      await expect(dialog(page).getByText('restore-old.bin', { exact: true })).toBeVisible();
      const deleted = responded(page, 'DELETE');
      await dialog(page).getByRole('button', { name: 'Cancel', exact: true }).click();
      await deleted;
      await openNewTicket(page);
      await dialog(page).getByRole('textbox', { name: 'Ticket Title' }).fill('Synthetic newer restore draft');
      const staged = responded(page, 'PUT');
      await stage(page, [{ name: 'restore-new.bin', bytes: [9, 2] }]);
      await staged;
      const read = responded(page, 'GET');
      hold.release();
      await read;
      await expect(dialog(page).getByRole('textbox', { name: 'Ticket Title' })).toHaveValue(
        'Synthetic newer restore draft',
      );
      await expect(dialog(page).getByText('restore-old.bin', { exact: true })).toHaveCount(0);
      await expect(dialog(page).getByText('restore-new.bin', { exact: true })).toBeVisible();
      await expect(dialog(page).getByText("Couldn't", { exact: false })).toHaveCount(0);
      await createAndExpectStored(page, app, { 'restore-new.bin': [9, 2] });
    } finally {
      hold?.release();
      await app.stop();
    }
  });

  for (const phase of ['save', 'restore'] as const)
    test(`HS2-791W19 ignores a late Not working ${phase} after cancel and a new target`, async ({
      context,
      baseURL,
    }) => {
      const app = await desktopApp(context, baseURL!);
      let hold: ReturnType<typeof app.pauseBridge> | undefined;
      try {
        const original = await app.completedTicket('Synthetic original report target'),
          later = await app.completedTicket('Synthetic replacement report target');
        const page = await app.launch();
        await openNewTicket(page, true);
        await dialog(page).getByRole('button', { name: 'Cancel', exact: true }).click();
        let report = await openNotWorking(page, original);
        await report.getByRole('textbox').fill('Synthetic original report');
        if (phase === 'save') hold = app.pauseBridge((method) => method === 'PUT');
        const saved = responded(page, 'PUT'),
          persisted = phase === 'restore' ? sessionSaved(page, 'older-report.bin') : undefined;
        await report.getByLabel('Browse evidence attachments').setInputFiles({
          name: 'older-report.bin',
          mimeType: 'application/octet-stream',
          buffer: Buffer.from([2, 1]),
        });
        if (phase === 'save') await hold!.started;
        else {
          await saved;
          await persisted;
          hold = app.pauseBridge(
            (method, key) => method === 'GET' && key.startsWith(`not-working:${app.server.checkoutId}:`),
          );
          await app.addProject(page);
          await page.getByRole('tab', { name: 'drafts', exact: true }).click();
          await hold.started;
          report = page.getByRole('dialog', { name: `Not Working — ${original.slug}` });
        }
        await report.getByRole('button', { name: 'Cancel', exact: true }).click();
        const replacement = await openNotWorking(page, later);
        await replacement.getByRole('textbox').fill('Synthetic replacement report');
        // Release before staging the replacement when PUT is held; the old completion is fenced.
        hold!.release();
        if (phase === 'save') await saved;
        const fresh = responded(page, 'PUT');
        await replacement.getByLabel('Browse evidence attachments').setInputFiles({
          name: 'newer-report.bin',
          mimeType: 'application/octet-stream',
          buffer: Buffer.from([6, 8]),
        });
        await fresh;
        await expect(replacement.getByRole('textbox')).toHaveValue('Synthetic replacement report');
        await expect(replacement.getByText('older-report.bin', { exact: true })).toHaveCount(0);
        await expect(replacement.getByText('newer-report.bin', { exact: true })).toBeVisible();
        await expect(replacement.getByText("Couldn't", { exact: false })).toHaveCount(0);
        await replacement.getByRole('button', { name: 'Report Not Working', exact: true }).click();
        await expect.poll(() => findFile(app.server.store, 'newer-report.bin')).toBeDefined();
        await expect.poll(() => app.records()).toEqual([]);
      } finally {
        hold?.release();
        await app.stop();
      }
    });
  test('HS2-791W19 persists edits made during restore without a further user action', async ({ context, baseURL }) => {
    const app = await desktopApp(context, baseURL!);
    let refresh: ReturnType<typeof app.pauseApi> | undefined, restore: ReturnType<typeof app.pauseBridge> | undefined;
    try {
      const page = await app.launch();
      await openNewTicket(page, true);
      await dialog(page).getByRole('textbox', { name: 'Ticket Title' }).fill('Synthetic recovery persistence');
      const saved = responded(page, 'PUT'),
        persisted = sessionSaved(page, 'before-restore.bin');
      await stage(page, [{ name: 'before-restore.bin', bytes: [1, 2] }]);
      await saved;
      await persisted;
      await app.addProject(page);
      refresh = app.pauseApi((method, path) => method === 'GET' && path === '/commands');
      restore = app.pauseBridge(
        (method, key) => method === 'GET' && key.startsWith(`composer:${app.server.checkoutId}:`),
      );
      await page.getByRole('tab', { name: 'drafts', exact: true }).click();
      await refresh.started;
      await openNewTicket(page);
      refresh.release();
      await restore.started;
      await dialog(page).getByRole('button', { name: 'Remove before-restore.bin' }).click();
      const added = responded(page, 'PUT');
      await stage(page, [{ name: 'during-restore.bin', bytes: [4, 2] }]);
      await added;
      const committed = sessionSaved(page, 'during-restore.bin');
      restore.release();
      await committed;
      const current = JSON.parse(await app.storedSession()).composer;
      expect(current.attachments.map((item: { name: string }) => item.name)).toEqual(['during-restore.bin']);
      await createAndExpectStored(page, app, { 'during-restore.bin': [4, 2] });
    } finally {
      refresh?.release();
      restore?.release();
      await app.stop();
    }
  });

  for (const fails of [false, true])
    test(`HS2-791W19 keeps a note-only report submitted during pending session restore on ${fails ? 'failure' : 'success'}`, async ({
      context,
      baseURL,
    }) => {
      const app = await desktopApp(context, baseURL!);
      let detail: ReturnType<typeof app.pauseApi> | undefined, reportHold: ReturnType<typeof app.pauseApi> | undefined;
      try {
        const ticket = await app.completedTicket('Synthetic selected completed ticket');
        const page = await app.launch();
        await openNewTicket(page, true);
        await dialog(page).getByRole('button', { name: 'Cancel', exact: true }).click();
        await page.locator(`[data-ticket-slug="${ticket.slug}"]`).first().click();
        const report = await openNotWorking(page, ticket);
        const persisted = sessionSaved(page, 'Synthetic saved note-only report');
        await report.getByRole('textbox').fill('Synthetic saved note-only report');
        await persisted;
        await app.addProject(page);
        detail = app.pauseApi(
          (method, path) => method === 'GET' && path.includes('/tickets/') && !path.endsWith('/tickets'),
        );
        await page.getByRole('tab', { name: 'drafts', exact: true }).click();
        await detail.started;
        const restored = page.getByRole('dialog', { name: `Not Working — ${ticket.slug}` });
        await restored.getByRole('textbox').fill('Synthetic edited note-only report');
        reportHold = app.pauseApi((method, path) => method === 'POST' && path.endsWith('/not-working'));
        app.failApiWhen((method, path) => fails && method === 'POST' && path.endsWith('/not-working'));
        await restored.getByRole('button', { name: 'Report Not Working', exact: true }).click();
        await reportHold.started;
        detail.release();
        // Wait for restore's selected-ticket response and the next render before releasing the submission.
        await page.evaluate(
          () =>
            new Promise<void>((resolve) =>
              requestAnimationFrame(() => {
                requestAnimationFrame(() => {
                  resolve();
                });
              }),
            ),
        );
        reportHold.release();
        if (fails) {
          await expect(restored.getByRole('textbox')).toHaveValue('Synthetic edited note-only report');
          await expect(restored.getByText('Synthetic operation rejected.', { exact: false })).toBeVisible();
          await expect(restored.getByRole('button', { name: 'Report Not Working', exact: true })).toBeEnabled();
        } else {
          await expect(restored.getByRole('textbox')).toBeHidden();
          await expect.poll(async () => JSON.parse(await app.storedSession()).notWorking).toBeUndefined();
        }
      } finally {
        detail?.release();
        reportHold?.release();
        await app.stop();
      }
    });
  test('HS2-791W19 saves current draft refs on pagehide before close while restore GET is held', async ({
    context,
    baseURL,
  }) => {
    const app = await desktopApp(context, baseURL!);
    let refresh: ReturnType<typeof app.pauseApi> | undefined, restore: ReturnType<typeof app.pauseBridge> | undefined;
    try {
      let page = await app.launch();
      await openNewTicket(page, true);
      await dialog(page).getByRole('textbox', { name: 'Ticket Title' }).fill('Synthetic close-time restore');
      const saved = responded(page, 'PUT'),
        persisted = sessionSaved(page, 'before-close.bin');
      await stage(page, [{ name: 'before-close.bin', bytes: [1, 2] }]);
      await saved;
      await persisted;
      await app.addProject(page);
      refresh = app.pauseApi((method, path) => method === 'GET' && path === '/commands');
      restore = app.pauseBridge(
        (method, key) => method === 'GET' && key.startsWith(`composer:${app.server.checkoutId}:`),
      );
      await page.getByRole('tab', { name: 'drafts', exact: true }).click();
      await refresh.started;
      await openNewTicket(page);
      refresh.release();
      await restore.started;
      await dialog(page).getByRole('button', { name: 'Remove before-close.bin' }).click();
      const added = responded(page, 'PUT');
      await stage(page, [{ name: 'during-close.bin', bytes: [9, 8] }]);
      await added;
      await expect(dialog(page).getByText('during-close.bin', { exact: true })).toBeVisible();
      // Real navigation delivers pagehide before the renderer closes; the bridge is real HTTP.
      await page.goto('about:blank');
      await expect
        .poll(async () =>
          JSON.parse(await app.storedSession()).composer.attachments.map((item: { name: string }) => item.name),
        )
        .toEqual(['during-close.bin']);
      await page.close();
      restore.release();
      page = await app.launch();
      await page.getByRole('button', { name: 'New ticket…' }).click();
      await expect(dialog(page).getByText('during-close.bin', { exact: true })).toBeVisible();
      await expect(dialog(page).getByText('before-close.bin', { exact: true })).toHaveCount(0);
      await createAndExpectStored(page, app, { 'during-close.bin': [9, 8] });
    } finally {
      refresh?.release();
      restore?.release();
      await app.stop();
    }
  });
});
