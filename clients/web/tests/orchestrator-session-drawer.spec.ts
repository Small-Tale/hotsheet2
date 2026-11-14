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
