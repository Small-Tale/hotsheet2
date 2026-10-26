import { expect, type Page, test } from '@playwright/test';

import type { FullTicket } from '../src/api';
import { type DesktopCloseHost } from '../src/desktop-close-contract';
import { desktopApp, dialog, openNewTicket, sessionSaved, stage } from './desktop-draft-files-fixture';
import { expectProjectSidebarLayout } from './project-sidebar-layout';

async function captureCloseState(page: Page, browser: string, state: string) {
  for (const [label, width] of [
    ['wide', 1280],
    ['medium', 900],
    ['narrow', 390],
    ['wide-again', 1280],
  ] as const) {
    await page.setViewportSize({ width, height: 900 });
    await expectProjectSidebarLayout(page, width >= 1024);
    if (width < 1024 && (state === 'correction-inspector-quota' || state === 'correction-revert-quota')) {
      const show = page.getByRole('button', { name: 'Show Ticket Inspector', exact: true });
      if (await show.isVisible()) await show.click();
      await expect(page.locator('#app-right-rail').getByRole('textbox', { name: 'Ticket details' })).toBeVisible();
    }
    await page.waitForFunction(() =>
      document
        .getAnimations()
        .every(
          (animation) =>
            animation.playState !== 'running' || animation.effect?.getComputedTiming().endTime === Infinity,
        ),
    );
    if (state === 'correction-inspector-quota' || state === 'correction-revert-quota') {
      const bounds = await page
        .locator('#app-right-rail')
        .getByRole('textbox', { name: 'Ticket details' })
        .boundingBox();
      expect(bounds?.x ?? -1).toBeGreaterThanOrEqual(0);
      expect((bounds?.x ?? -1) + (bounds?.width ?? 0)).toBeLessThanOrEqual(width + 1);
    }
    await page.screenshot({ path: `target/visual-captures/hs2-e0h7s0-${browser}-${state}-${label}.png` });
  }
}

for (const surface of ['inspector', 'reader', 'linked-reader', 'revert'] as const) {
  test(`captures ${surface} latest recovery behind a held save, refuses quota and restores after retry (HS2-E0H7S0)`, async ({
    context,
    baseURL,
    browserName,
  }) => {
    const acknowledgements: { request: string; success: boolean }[] = [];
    await context.exposeFunction('recordDesktopClose', (value: { request: string; success: boolean }) => {
      acknowledgements.push(value);
    });
    await context.addInitScript(() => {
      const host = window as unknown as DesktopCloseHost & { recordDesktopClose(value: unknown): Promise<void> };
      host.__TAURI_INTERNALS__ = { invoke: async (_command, value) => host.recordDesktopClose(value) };
    });
    const app = await desktopApp(context, baseURL!);
    let release = () => {};
    try {
      const ticket = await app.server.request<FullTicket>(`/checkouts/${app.server.checkoutId}/tickets`, 'POST', {
        title: 'Controlled recovery target',
        category: 'task',
        details: 'Synthetic edit base',
        up_next: true,
      });
      const source =
        surface === 'linked-reader'
          ? await app.server.request<FullTicket>(`/checkouts/${app.server.checkoutId}/tickets`, 'POST', {
              title: 'Controlled recovery source',
              category: 'task',
              details: `Open @${app.server.checkoutId}/${ticket.slug}`,
              up_next: true,
            })
          : ticket;
      let page = await app.launch();
      page.on('dialog', (notice) => void notice.accept());
      await openNewTicket(page, true);
      await dialog(page).getByRole('button', { name: 'Cancel', exact: true }).click();
      const openEditor = async () => {
        const row = page.locator(`[data-component="summary-row"][data-ticket-slug="${source.slug}"]`);
        if (surface === 'reader') await row.dblclick();
        else await row.click();
        if (surface === 'linked-reader')
          await page
            .locator('#app-right-rail')
            .getByRole('link', { name: `@${app.server.checkoutId}/${ticket.slug}` })
            .click();
        const parent =
          surface === 'inspector' || surface === 'revert'
            ? page.locator('#app-right-rail')
            : page.locator('[data-component="ticket-reader"]').last();
        await parent.locator('[data-action="edit-markdown"]').click({ position: { x: 4, y: 4 } });
        const editor = parent.getByRole('textbox', { name: 'Ticket details' });
        await expect(editor).toBeFocused();
        return { parent, editor };
      };
      let { parent, editor } = await openEditor();
      const held = app.pauseApi((method, path) => method === 'PATCH' && path.includes('/tickets/'));
      release = held.release;
      await editor.fill('Older submitted edit');
      await editor.press('Tab');
      await held.started;
      const latest = surface === 'revert' ? 'Synthetic edit base' : 'Newest controlled-close edit';
      await editor.fill(latest);
      // Fail only the per-ticket recovery copy; the unrelated session snapshot remains healthy.
      await page.evaluate(() => {
        const before = Object.getOwnPropertyDescriptor(Storage.prototype, 'setItem')?.value as Storage['setItem'];
        Storage.prototype.setItem = function (key, value) {
          if (key.startsWith('hotsheet.ticket-draft:')) throw new DOMException('Synthetic quota', 'QuotaExceededError');
          before.call(this, key, value);
        };
        Object.assign(window, {
          restoreRecoveryStorage: () => {
            Storage.prototype.setItem = before;
          },
        });
        (window as unknown as DesktopCloseHost).__HOTSHEET_DESKTOP_CLOSE__!('1');
      });
      await expect.poll(() => acknowledgements).toEqual([{ request: '1', success: false }]);
      await expect(page.locator('html')).toHaveJSProperty('inert', false);
      await expect(editor).toHaveValue(latest);
      await captureCloseState(page, browserName, `correction-${surface}-quota`);
      await page.evaluate(() => {
        (window as unknown as { restoreRecoveryStorage(): void }).restoreRecoveryStorage();
        (window as unknown as DesktopCloseHost).__HOTSHEET_DESKTOP_CLOSE__!('2');
        (window as unknown as DesktopCloseHost).__HOTSHEET_DESKTOP_CLOSE__!('2');
      });
      // The older server response is still held: only durable local recovery authorizes this ACK.
      await expect
        .poll(() => acknowledgements)
        .toEqual([
          { request: '1', success: false },
          { request: '2', success: true },
        ]);
      release();
      page = await app.launch(page);
      ({ parent, editor } = await openEditor());
      await expect(editor).toHaveValue(latest);
      await expect(parent).toBeVisible();
      const saved = page.waitForResponse(
        (response) => response.request().method() === 'PATCH' && response.url().includes('/tickets/') && response.ok(),
      );
      await editor.fill('Saved after recovery');
      await editor.press('Tab');
      await saved;
      await parent.locator('[data-action="edit-markdown"]').click({ position: { x: 4, y: 4 } });
      await expect(editor).toHaveValue('Saved after recovery');
      // Ordinary inline Escape finishes via focusout; it does not discard text (HS2-Q2T01A).
      const escaped = page.waitForResponse(
        (response) => response.request().method() === 'PATCH' && response.url().includes('/tickets/') && response.ok(),
      );
      await editor.fill('Finished with Escape after recovery');
      await editor.press('Escape');
      await escaped;
      await page.evaluate(() => {
        (window as unknown as DesktopCloseHost).__HOTSHEET_DESKTOP_CLOSE__!('1');
      });
      await expect.poll(() => acknowledgements).toHaveLength(3);
      expect(acknowledgements.at(-1)).toEqual({ request: '1', success: true });
      page = await app.launch(page);
      ({ editor } = await openEditor());
      await expect(editor).toHaveValue('Finished with Escape after recovery');
    } finally {
      release();
      await app.stop();
    }
  });
}

// Real parent + real HTTP storage. IPC is a browser adapter here; native lifecycle evidence is separate.
test('captures large unblurred composer recovery behind a held write before desktop close ACK (HS2-E0H7S0)', async ({
  context,
  baseURL,
  browserName,
}) => {
  const acknowledgements: { request: string; success: boolean }[] = [];
  await context.exposeFunction('recordDesktopClose', (value: { request: string; success: boolean }) => {
    acknowledgements.push(value);
  });
  await context.addInitScript(() => {
    const host = window as unknown as DesktopCloseHost & { recordDesktopClose(value: unknown): Promise<void> };
    host.__TAURI_INTERNALS__ = { invoke: async (_command, value) => host.recordDesktopClose(value) };
  });
  const app = await desktopApp(context, baseURL!);
  let release = () => {};
  try {
    let page = await app.launch();
    await openNewTicket(page, true);
    const title = dialog(page).getByRole('textbox', { name: 'Ticket Title' });
    await expect(title).toBeFocused();
    const saved = sessionSaved(page, 'Large controlled-close draft');
    await title.fill('Large controlled-close draft');
    await stage(page, [{ name: 'kept-native-proof.bin', bytes: [0, 255, 128] }]);
    await saved;
    await expect.poll(() => app.records()).toHaveLength(1);
    let held!: () => void;
    const started = new Promise<void>((resolve) => {
        held = resolve;
      }),
      resumed = new Promise<void>((resolve) => {
        release = resolve;
      });
    await page.route('**/__hotsheet/desktop-storage', async (route) => {
      if (route.request().method() === 'PUT' && route.request().postDataJSON().key === 'hotsheet.close-held') {
        held();
        await resumed;
      }
      await route.continue();
    });
    await page.evaluate(() => {
      localStorage.setItem('hotsheet.close-held', '界'.repeat(24000));
    });
    await started;
    const details = '界'.repeat(32000);
    await dialog(page).getByRole('textbox', { name: 'Details' }).fill(details);
    await page.evaluate(() => {
      const host = window as unknown as DesktopCloseHost;
      host.__HOTSHEET_DESKTOP_CLOSE__!('1');
      host.__HOTSHEET_DESKTOP_CLOSE__!('1');
    });
    expect(acknowledgements).toEqual([]);
    await expect(page.locator('html')).toHaveJSProperty('inert', true);
    await captureCloseState(page, browserName, 'waiting');
    release();
    await expect.poll(() => acknowledgements).toEqual([{ request: '1', success: true }]);
    const stored = JSON.parse(await app.storedSession()) as {
      composer: { title: string; details: string; attachments: unknown[] };
    };
    expect(stored.composer.title).toBe('Large controlled-close draft');
    expect(stored.composer.details).toBe(details);
    expect(stored.composer.attachments).toHaveLength(1);
    page = await app.launch(page);
    // Startup restores the draft without interruptively opening it (initialProjectRestoreComplete=false).
    await openNewTicket(page);
    await expect(dialog(page).getByRole('textbox', { name: 'Details' })).toHaveValue(details);
    await expect(dialog(page).getByText('kept-native-proof.bin', { exact: true })).toBeVisible();
  } finally {
    release();
    await context.unrouteAll({ behavior: 'ignoreErrors' });
    await app.stop();
  }
});

test('keeps a failed large close open, saves ordinary preferences, and retries retained recovery (HS2-E0H7S0)', async ({
  context,
  baseURL,
  browserName,
}) => {
  const acknowledgements: { request: string; success: boolean }[] = [];
  await context.exposeFunction('recordDesktopClose', (value: { request: string; success: boolean }) => {
    acknowledgements.push(value);
  });
  await context.addInitScript(() => {
    const host = window as unknown as DesktopCloseHost & { recordDesktopClose(value: unknown): Promise<void> };
    host.__TAURI_INTERNALS__ = { invoke: async (_command, value) => host.recordDesktopClose(value) };
  });
  const app = await desktopApp(context, baseURL!);
  try {
    let page = await app.launch();
    page.on('dialog', (notice) => void notice.accept());
    await openNewTicket(page, true);
    await expect(dialog(page).getByRole('textbox', { name: 'Ticket Title' })).toBeFocused();
    await dialog(page).getByRole('textbox', { name: 'Ticket Title' }).fill('Retained after failed close');
    const details = '证'.repeat(32000);
    await dialog(page).getByRole('textbox', { name: 'Details' }).fill(details);
    let fail = true;
    await page.route('**/__hotsheet/desktop-storage', async (route) => {
      if (
        fail &&
        route.request().method() === 'PUT' &&
        String(route.request().postDataJSON().value).includes(details)
      ) {
        await route.fulfill({ status: 503, body: 'Synthetic write failure' });
      } else await route.continue();
    });
    await page.evaluate(() => {
      (window as unknown as DesktopCloseHost).__HOTSHEET_DESKTOP_CLOSE__!('1');
    });
    await expect.poll(() => acknowledgements).toEqual([{ request: '1', success: false }]);
    await expect(page.locator('html')).toHaveJSProperty('inert', false);
    await expect(dialog(page).getByRole('textbox', { name: 'Details' })).toHaveValue(details);
    await captureCloseState(page, browserName, 'failed');
    const preference = page.waitForResponse(
      (response) =>
        response.request().method() === 'PUT' &&
        response.url().endsWith('/__hotsheet/desktop-storage') &&
        response.request().postDataJSON().key === 'hotsheet.synthetic-preference' &&
        response.status() === 204,
    );
    await page.evaluate(() => {
      localStorage.setItem('hotsheet.synthetic-preference', 'kept');
    });
    await preference;
    fail = false;
    await page.evaluate(() => {
      (window as unknown as DesktopCloseHost).__HOTSHEET_DESKTOP_CLOSE__!('2');
    });
    await expect
      .poll(() => acknowledgements)
      .toEqual([
        { request: '1', success: false },
        { request: '2', success: true },
      ]);
    page = await app.launch(page);
    await openNewTicket(page);
    await expect(dialog(page).getByRole('textbox', { name: 'Details' })).toHaveValue(details);
    expect(await page.evaluate(() => localStorage.getItem('hotsheet.synthetic-preference'))).toBe('kept');
  } finally {
    await context.unrouteAll({ behavior: 'ignoreErrors' });
    await app.stop();
  }
});
