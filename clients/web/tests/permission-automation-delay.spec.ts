import { expect, test } from '@playwright/test';

import { desktopApp, dialog, openNewTicket } from './desktop-draft-files-fixture';

for (const action of ['allow', 'deny'] as const) {
  test(`persists 5 seconds and automatically ${action === 'allow' ? 'allows' : 'denies'} a real permission request (HS2-0XF17D)`, async ({
    context,
    baseURL,
  }) => {
    const app = await desktopApp(context, baseURL!);
    try {
      const page = await app.launch();
      expect(
        await app.server.request<{ configured: boolean; action: string }>(
          `/checkouts/${app.server.checkoutId}/permission-automation`,
        ),
      ).toMatchObject({ configured: false, action: 'off' });
      await page.clock.install();
      await openNewTicket(page, true);
      await dialog(page).getByRole('button', { name: 'Cancel', exact: true }).click();
      const openSettings = async () => {
        await page.getByLabel('Settings View').click();
        await page.getByRole('button', { name: 'Permissions', exact: true }).click();
      };
      await openSettings();
      const decision = page.locator('kui-select[name="permission-automation-action"]'),
        delay = page.locator('kui-select[name="permission-automation-delay"]'),
        stored = async () => {
          const saved = await app.server.request<{ configured: boolean; action: string; delayMs: number }>(
            `/checkouts/${app.server.checkoutId}/permission-automation`,
          );
          return saved.configured ? JSON.stringify({ action: saved.action, delayMs: saved.delayMs }) : null;
        },
        choose = async (control: typeof decision, label: string) => {
          await control.getByRole('combobox').click();
          await control.getByRole('option', { name: label, exact: true }).click();
        };
      await choose(decision, action === 'allow' ? 'Auto-Allow' : 'Auto-Deny');
      await choose(delay, '5 seconds');
      await expect.poll(stored).toBe(JSON.stringify({ action, delayMs: 5000 }));
      await expect(delay).toHaveJSProperty('value', '5000');
      await page.reload();
      await openSettings();
      await expect(decision).toHaveJSProperty('value', action);
      await expect(delay).toHaveJSProperty('value', '5000');
      // A fresh desktop origin has no browser record for this project. Its choice
      // comes from the project-local file through the real checkout route.
      await app.projectRemembered();
      const freshOrigin = await app.launch();
      expect(
        await freshOrigin.evaluate(
          (key) => localStorage.getItem(key),
          `hotsheet.project.${app.server.checkoutId}.permission-automation`,
        ),
      ).toBeNull();
      await freshOrigin.getByLabel('Settings View').click();
      await freshOrigin.getByRole('button', { name: 'Permissions', exact: true }).click();
      await expect(freshOrigin.locator('kui-select[name="permission-automation-action"]')).toHaveJSProperty(
        'value',
        action,
      );
      await freshOrigin.close();
      // Reset and edit again through the actual controls, preserving the chosen delay.
      await choose(decision, 'Off');
      await expect(delay.getByRole('combobox')).toBeDisabled();
      await choose(decision, action === 'allow' ? 'Auto-Allow' : 'Auto-Deny');
      await choose(delay, '15 seconds');
      await choose(delay, '5 seconds');
      await expect.poll(stored).toBe(JSON.stringify({ action, delayMs: 5000 }));
      if (action === 'allow') {
        for (const [name, width] of [
          ['wide', 1440],
          ['medium', 900],
          ['narrow', 390],
          ['wide-again', 1440],
        ] as const) {
          await page.setViewportSize({ width, height: 900 });
          await delay.getByRole('combobox').click();
          await expect(delay.getByRole('option', { name: '5 seconds', exact: true })).toBeInViewport();
          await page.waitForFunction(() =>
            document
              .getAnimations()
              .every(
                (animation) =>
                  animation.playState !== 'running' || animation.effect?.getComputedTiming().iterations === Infinity,
              ),
          );
          await page.screenshot({ path: `target/visual-captures/hs2-qagsjk-${name}.png` });
          await page.keyboard.press('Escape');
        }
      }
      // The real authenticated permission route waits until the visible timer answers it.
      const ask = app.server.request<{ decision: string }>('/permissions/ask', 'POST', {
        project: app.server.root,
        connection: 'synthetic-five-second-hook',
        tool: 'Bash',
        action: 'echo synthetic-five-second-permission',
      });
      const popup = page.locator('[data-component="permission-request-popup"]');
      await expect(popup).toBeVisible();
      await expect(popup).toContainText('synthetic-five-second-permission');
      await page.clock.runFor(6000);
      expect(await ask).toEqual({ decision: action });
      await expect(popup).toHaveCount(0);
    } finally {
      await context.unrouteAll({ behavior: 'ignoreErrors' });
      await app.stop();
    }
  });
}

test('HS2-QAGSJK synchronizes a saved automatic decision between same-origin windows', async ({ context, baseURL }) => {
  const app = await desktopApp(context, baseURL!);
  try {
    const first = await app.launch();
    await openNewTicket(first, true);
    await dialog(first).getByRole('button', { name: 'Cancel', exact: true }).click();
    await app.projectRemembered();
    await first.getByLabel('Settings View').click();
    await first.getByRole('button', { name: 'Permissions', exact: true }).click();
    const firstDecision = first.locator('kui-select[name="permission-automation-action"]');
    await firstDecision.getByRole('combobox').click();
    await firstDecision.getByRole('option', { name: 'Auto-Allow', exact: true }).click();
    await expect
      .poll(
        async () =>
          (await app.server.request<{ action: string }>(`/checkouts/${app.server.checkoutId}/permission-automation`))
            .action,
      )
      .toBe('allow');

    const second = await context.newPage();
    await second.goto(first.url());
    await second.getByLabel('Settings View').click();
    await second.getByRole('button', { name: 'Permissions', exact: true }).click();
    const secondDecision = second.locator('kui-select[name="permission-automation-action"]');
    await expect(secondDecision).toHaveJSProperty('value', 'allow');
    await secondDecision.getByRole('combobox').click();
    await secondDecision.getByRole('option', { name: 'Off', exact: true }).click();
    await expect(firstDecision).toHaveJSProperty('value', 'off');
    await second.close();
  } finally {
    await context.unrouteAll({ behavior: 'ignoreErrors' });
    await app.stop();
  }
});

test('HS2-QAGSJK restores the confirmed decision when the local write fails', async ({ context, baseURL }) => {
  const app = await desktopApp(context, baseURL!);
  try {
    const page = await app.launch();
    await openNewTicket(page, true);
    await dialog(page).getByRole('button', { name: 'Cancel', exact: true }).click();
    await page.getByLabel('Settings View').click();
    await page.getByRole('button', { name: 'Permissions', exact: true }).click();
    const decision = page.locator('kui-select[name="permission-automation-action"]');
    await decision.getByRole('combobox').click();
    await decision.getByRole('option', { name: 'Auto-Allow', exact: true }).click();
    await expect
      .poll(
        async () =>
          (await app.server.request<{ action: string }>(`/checkouts/${app.server.checkoutId}/permission-automation`))
            .action,
      )
      .toBe('allow');
    let rejected = 0;
    await page.route('**/permission-automation', (route) => {
      if (route.request().method() !== 'PUT') return route.fallback();
      rejected += 1;
      return route.fulfill({ status: 500, json: { error: 'synthetic write failure' } });
    });
    await decision.getByRole('combobox').click();
    await decision.getByRole('option', { name: 'Auto-Deny', exact: true }).click();
    await expect.poll(() => rejected).toBe(1);
    await expect(decision).toHaveJSProperty('value', 'allow');
    expect(
      (await app.server.request<{ action: string }>(`/checkouts/${app.server.checkoutId}/permission-automation`))
        .action,
    ).toBe('allow');
  } finally {
    await context.unrouteAll({ behavior: 'ignoreErrors' });
    await app.stop();
  }
});
