import { mkdir, writeFile } from 'node:fs/promises';
import { join } from 'node:path';

import { expect, test } from '@playwright/test';

import type { TerminalInfo } from '../src/api';
import { desktopApp } from './desktop-draft-files-fixture';

const TOOL = 'hs-synthetic-agent';
const RUN = '01ARZ3NDEKTSV4RRFFQ69G5FB0';

/**
 * A synthetic AI tool in the fixture server's own plugin directory: it launches the Node binary
 * that runs this spec and holds, so no real AI tool is started or probed.
 */
async function syntheticTool(home: string) {
  const plugin = join(home, 'plugins', TOOL);
  await mkdir(plugin, { recursive: true });
  await writeFile(join(plugin, 'instructions.md'), 'Synthetic agent\n');
  await writeFile(
    join(plugin, 'manifest.toml'),
    [
      `id = "${TOOL}"`,
      `display_name = "${TOOL}"`,
      'product_name = "Synthetic agent"',
      'tier = "cli-agent"',
      // Detected through the Node binary already on PATH; driven by spawning it.
      '[detection]',
      'binaries = ["node"]',
      '[drive]',
      'transport = "spawn"',
      `program = ${JSON.stringify(process.execPath)}`,
      'args = []',
      'content = "arg"',
      '[instructions]',
      'target = "AGENTS.md"',
      'section = "instructions.md"',
      '[mcp]',
      'target = ".mcp.json"',
      'format = "claude-json"',
      'server_name = "hotsheet"',
      'command = "hotsheet-mcp"',
      'args = ["--path", "{store}"]',
      '[launch]',
      `program = ${JSON.stringify(process.execPath)}`,
      `args = ["-e", "process.stdout.write('ready\\\\n'); setInterval(() => {}, 1000000)"]`,
      '',
    ].join('\n'),
  );
}

// HS2-KTHJN2: a session an orchestrator starts on this machine is an ordinary AI terminal of its
// drawer, named by the saved tab name `<orchestration project>: run <last eight of the run id>`
// (docs/26 §26.16). The start goes through the hosted route an orchestrator calls.
test('labels an orchestrator-started session in the real drawer (HS2-KTHJN2)', async ({ context, baseURL }) => {
  test.setTimeout(150_000);
  // Seeded before the server starts, so its discovery finds the tool.
  const app = await desktopApp(context, baseURL!, {
    canonicalRoot: true,
    seed: async (fixture) => syntheticTool(fixture.env.HOTSHEET_HOME!),
  });
  try {
    const started = await app.server.request<{
      outcome: string;
      reason?: string;
      session?: { label?: string; session: { session_id: string } };
    }>(`/checkouts/${app.server.checkoutId}/target-sessions`, 'POST', {
      session: {
        operation_id: 'sched-start-drawer',
        tool: TOOL,
        origin: { owner_checkout_id: app.server.checkoutId, project: 'Release train', run_id: RUN },
      },
      // The machine's own load must not refuse the start on a busy test host.
      limits: { max_sessions: 64, min_free_ram_mb: 0, max_cpu_percent: 100, min_free_disk_mb: 0 },
      actor: { role: 'human', id: 'maintainer' },
    });
    expect(started, JSON.stringify(started)).toMatchObject({ outcome: 'started' });
    const terminal = started.session!.session.session_id;
    const label = `Release train: run ${RUN.slice(-8)}`;
    const listed = (await app.server.request<TerminalInfo[]>('/terminals')).find((row) => row.id === terminal);
    expect(started.session?.label).toBe(label);
    expect(listed?.name).toBe(label);

    const page = await app.launch();
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.getByRole('button', { name: 'Open Project' }).click();
    await page.getByRole('button', { name: 'Local Project' }).click();
    await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();
    await page.getByRole('button', { name: 'Show Terminal Drawer' }).click();
    const drawer = page.locator('[data-component="terminal-drawer"]');
    const tab = drawer.locator(`[data-tab-kind="terminal"][data-terminal-id="${terminal}"]`);
    await expect(tab).toBeVisible();
    await expect(tab).toContainText(label);
    for (const scheme of ['light', 'dark'] as const) {
      await page.emulateMedia({ colorScheme: scheme });
      for (const width of [1440, 1100, 390, 1440]) {
        await page.setViewportSize({ width, height: 900 });
        await page.waitForFunction(() =>
          document.getAnimations().every((animation) => animation.playState !== 'running'),
        );
        // At every width the tab names the orchestration project and the run.
        await expect(tab).toBeVisible();
        await expect(tab).toContainText(label);
        await page.screenshot({ path: `target/visual-captures/hs2-kthjn2-drawer-${scheme}-${width}.png` });
      }
    }
  } finally {
    await app.stop();
  }
});

// HS2-6MNMVB: a person at the hosting machine takes an orchestrator-started session over from its
// drawer tab. The tab's context menu offers Take Over from Orchestrator and Take Over and Pause
// while the orchestrator owns it, records the takeover through the hosted route, and stops
// offering them once the session is the person's.
test('takes an orchestrator session over from its drawer tab (HS2-6MNMVB)', async ({ context, baseURL }) => {
  test.setTimeout(150_000);
  const app = await desktopApp(context, baseURL!, {
    canonicalRoot: true,
    seed: async (fixture) => syntheticTool(fixture.env.HOTSHEET_HOME!),
  });
  try {
    const hosted = `/checkouts/${app.server.checkoutId}/target-sessions`;
    const started = await app.server.request<{ outcome: string; session?: { session: { session_id: string } } }>(
      hosted,
      'POST',
      {
        session: {
          operation_id: 'sched-start-takeover',
          tool: TOOL,
          origin: { owner_checkout_id: app.server.checkoutId, project: 'Release train', run_id: RUN },
        },
        limits: { max_sessions: 64, min_free_ram_mb: 0, max_cpu_percent: 100, min_free_disk_mb: 0 },
        actor: { role: 'human', id: 'maintainer' },
      },
    );
    expect(started, JSON.stringify(started)).toMatchObject({ outcome: 'started' });
    const terminal = started.session!.session.session_id;
    const owner = async () =>
      (
        await app.server.request<{ sessions: { session: { owner: string; paused: boolean; operation_id: string } }[] }>(
          hosted,
        )
      ).sessions.find((item) => item.session.operation_id === 'sched-start-takeover')?.session;
    expect(await owner()).toMatchObject({ owner: 'orchestrator', paused: false });

    const page = await app.launch();
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.getByRole('button', { name: 'Open Project' }).click();
    await page.getByRole('button', { name: 'Local Project' }).click();
    await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();
    await page.getByRole('button', { name: 'Show Terminal Drawer' }).click();
    const tab = page
      .locator('[data-component="terminal-drawer"]')
      .locator(`[data-tab-kind="terminal"][data-terminal-id="${terminal}"]`);
    await expect(tab).toBeVisible();
    const menu = page.locator('[data-context-menu="app-tab"]');
    for (const width of [1440, 1100, 390]) {
      await page.setViewportSize({ width, height: 900 });
      await page.waitForFunction(() =>
        document.getAnimations().every((animation) => animation.playState !== 'running'),
      );
      await tab.click({ button: 'right' });
      await expect(menu.getByRole('menuitem', { name: 'Take Over from Orchestrator' })).toBeVisible();
      await expect(menu.getByRole('menuitem', { name: 'Take Over and Pause' })).toBeVisible();
      await expect(menu.getByRole('menuitem', { name: 'Take Over and Pause' })).toBeInViewport({ ratio: 1 });
      await page.screenshot({ path: `target/visual-captures/hs2-6mnmvb-menu-${width}.png` });
      await page.keyboard.press('Escape');
      await expect(menu).toHaveCount(0);
    }
    await page.setViewportSize({ width: 1440, height: 900 });
    await tab.click({ button: 'right' });
    await menu.getByRole('menuitem', { name: 'Take Over and Pause' }).click();
    await expect.poll(owner).toMatchObject({ owner: 'human', paused: true });
    // The drawer listing names the new owner.
    await expect
      .poll(
        async () =>
          (await app.server.request<TerminalInfo[]>('/terminals')).find((row) => row.id === terminal)?.orchestrator,
      )
      .toMatchObject({ owner: 'human', paused: true });
    // Once it is the person's, the menu no longer offers a takeover.
    await expect(async () => {
      if (await menu.count()) await page.keyboard.press('Escape');
      await expect(menu).toHaveCount(0);
      await tab.click({ button: 'right' });
      await expect(menu.getByRole('menuitem', { name: 'Rename…' })).toBeVisible();
      await expect(menu.getByRole('menuitem', { name: /Take Over/u })).toHaveCount(0, { timeout: 1_000 });
    }).toPass();
    await page.waitForFunction(() => document.getAnimations().every((animation) => animation.playState !== 'running'));
    await page.screenshot({ path: `target/visual-captures/hs2-6mnmvb-menu-after-1440.png` });
    await page.keyboard.press('Escape');
  } finally {
    await app.stop();
  }
});
