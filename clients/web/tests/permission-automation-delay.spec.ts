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
        stored = () =>
          page.evaluate(
            (key) => localStorage.getItem(key),
            `hotsheet.project.${app.server.checkoutId}.permission-automation`,
          ),
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
          await page.screenshot({ path: `target/visual-captures/hs2-0xf17d-${name}.png` });
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
