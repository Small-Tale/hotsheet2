import { expect, type Page, test } from '@playwright/test';

import { realTicketServer } from './real-ticket-server';
import { installTerminalFixture } from './terminal-feedback-fixture';

interface Limits {
  physical_memory_bytes: number | null;
  defaults: { history_lines: number; replay_bytes: number; warm_terminals: number };
  configured: { history_lines: number | null; replay_bytes: number | null; warm_terminals: number | null };
  effective: { history_lines: number; replay_bytes: number; warm_terminals: number };
}

async function choose(page: Page, name: string, value: string) {
  const select = page.locator(`wa-select[name="${name}"]`);
  await select.locator('[part~="combobox"]').click();
  await expect(select).toHaveJSProperty('open', true);
  await select.locator(`wa-option[value="${value}"]`).click();
  await expect(select).toHaveJSProperty('value', value);
}

// HS2-Y5DYC0: App Settings → Terminal memory reads and writes the real server's machine-wide limits,
// and an emulator mounted afterward keeps the chosen history.
test('saves machine-wide terminal limits and sizes terminals opened afterward', async ({ page }) => {
  const server = await realTicketServer();
  try {
    await page.setViewportSize({ width: 1280, height: 800 });
    await installTerminalFixture(page);
    // Only the limits route reaches the real server; the project and terminals stay fixtures.
    await page.route('**/terminal-limits', async (route) => {
      const response = await route.fetch({
        url: `${server.url}/terminal-limits`,
        headers: { ...route.request().headers(), 'X-Hotsheet-Secret': server.secret },
      });
      await route.fulfill({ response });
    });
    const initial = await server.request<Limits>('/terminal-limits');
    expect(initial.configured).toEqual({ history_lines: null, replay_bytes: null, warm_terminals: null });

    await page.goto('/');
    await page.getByRole('button', { name: 'Open Project' }).click();
    await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();
    await page.getByLabel('Settings View').click();
    await page.getByRole('button', { name: 'Terminal memory', exact: true }).click();
    const settings = page.locator('[data-component="terminal-memory-settings"]'),
      history = page.locator('wa-select[name="terminal-history-lines"]');
    await expect(settings.getByRole('heading', { name: 'Terminal memory' })).toBeVisible();
    await expect(history).toHaveJSProperty('value', 'default');
    await expect(history).toContainText(`Default (${initial.defaults.history_lines.toLocaleString('en-US')} lines)`);
    await page.screenshot({ path: 'target/visual-captures/hs2-y5dyc0-terminal-memory-wide.png', fullPage: true });

    await choose(page, 'terminal-history-lines', '10000');
    await expect
      .poll(async () => (await server.request<Limits>('/terminal-limits')).configured)
      .toEqual({ history_lines: 10_000, replay_bytes: null, warm_terminals: null });
    const saved = await server.request<Limits>('/terminal-limits');
    // The unset replay follows the configured history, so a reconnect restores what the client keeps.
    expect(saved.effective.replay_bytes).toBe(10_000 * 192);
    await expect(page.locator('wa-select[name="terminal-replay-bytes"]')).toContainText('Default (1.8 MB)');

    await choose(page, 'terminal-warm-terminals', '4');
    await expect.poll(async () => (await server.request<Limits>('/terminal-limits')).configured.warm_terminals).toBe(4);

    // A magnified card opened afterward keeps a fifth of the configured history.
    await page.getByRole('button', { name: 'Workspace Grid' }).click();
    const dashboard = page.getByRole('region', { name: 'Workspace Grid' }),
      tile = dashboard.locator('[data-component="terminal-tile"][data-terminal-key="terminal-feedback:nano"]');
    await tile.click();
    const magnified = dashboard.getByRole('dialog', { name: 'Magnified nano' });
    await expect(magnified.locator('[data-display-mode="interactive"]')).toHaveAttribute(
      'data-scrollback-limit',
      '2000',
    );
    await magnified.click({ position: { x: 5, y: 5 } });

    // Choosing Default clears the explicit history again.
    await page.getByRole('tab', { name: 'Terminal feedback' }).click();
    await page.getByLabel('Settings View').click();
    await page.getByRole('button', { name: 'Terminal memory', exact: true }).click();
    await expect(history).toHaveJSProperty('value', '10000');
    await choose(page, 'terminal-history-lines', 'default');
    await expect
      .poll(async () => (await server.request<Limits>('/terminal-limits')).configured)
      .toEqual({ history_lines: null, replay_bytes: null, warm_terminals: 4 });
  } finally {
    await server.stop();
  }
});

test('lays out every Terminal memory state without overflow at wide and phone widths', async ({ page }) => {
  for (const width of [1280, 390]) {
    await page.setViewportSize({ width, height: 900 });
    await page.goto('/ux-demo?component=terminal-memory-settings');
    const examples = page.locator('[data-component="terminal-memory-settings"]');
    await expect(examples).toHaveCount(6);
    await expect(page.locator('[data-role="terminal-limits-conservative"]')).toHaveCount(1);
    // The CLI-set 8,000-line history is listed and selected although it is not a preset.
    await expect(examples.nth(1).locator('wa-select[name="terminal-history-lines"]')).toHaveJSProperty('value', '8000');
    for (const example of await examples.all()) {
      const box = (await example.boundingBox())!;
      expect(box.x).toBeGreaterThanOrEqual(0);
      expect(box.x + box.width).toBeLessThanOrEqual(width);
    }
    expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(width);
    await page.screenshot({
      path: `target/visual-captures/hs2-y5dyc0-terminal-memory-demo-${width}.png`,
      fullPage: true,
    });
    for (const [index, state] of [
      [1, 'explicit'],
      [2, 'another-device'],
      [4, 'loading'],
      [5, 'failed'],
    ] as const)
      await examples.nth(index).screenshot({
        path: `target/visual-captures/hs2-y5dyc0-terminal-memory-${state}-${width}.png`,
      });
  }
});
