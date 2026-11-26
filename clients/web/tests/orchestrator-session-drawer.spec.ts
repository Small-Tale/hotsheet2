import { expect, test } from '@playwright/test';

import type { TerminalInfo } from '../src/api';
import { desktopApp } from './desktop-draft-files-fixture';
import { openMockedProject } from './open-mocked-project';
import { orchestrateSelf, scheduler, send, SYNTHETIC_TOOL, syntheticTool, TARGET } from './orchestration-real-fixtures';

const person = { role: 'human', id: 'maintainer' };

const TOOL = SYNTHETIC_TOOL;
const RUN = '01ARZ3NDEKTSV4RRFFQ69G5FB0';

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
    await openMockedProject(page);
    await page.getByRole('button', { name: 'Show Terminal Drawer' }).click();
    const drawer = page.locator('[data-component="terminal-drawer"]');
    const tab = drawer.locator(`[data-tab-kind="terminal"][data-terminal-id="${terminal}"]`);
    await expect(tab).toBeVisible();
    await expect(tab).toContainText(label);
    const caption = drawer.locator('[data-component="terminal-tile"] .terminal-tile__identity').first();
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
        // HS2-8KZF59: a capped tab ellipsizes the middle, so the run tail that tells two sessions of
        // one project apart stays wholly visible (HS2-M4YJM4: the tail alone, without `run `).
        await expect
          .poll(() =>
            tab.evaluate((element) => {
              const name = element.querySelector<HTMLElement>('.kui-tab-bar-item__name')!;
              const tail = element.querySelector<HTMLElement>('.kui-tab-bar-item__name-tail');
              if (!tail) return { tail: null };
              const nameRect = name.getBoundingClientRect(),
                tailRect = tail.getBoundingClientRect();
              return {
                tail: tail.textContent,
                tailClipped: tail.scrollWidth > tail.clientWidth + 1,
                tailInside: tailRect.left >= nameRect.left - 1 && tailRect.right <= nameRect.right + 1,
              };
            }),
          )
          .toEqual({ tail: RUN.slice(-8), tailClipped: false, tailInside: true });
        // HS2-3RBYJK: the Project Grid tile's `project › terminal` caption keeps the same suffix whole.
        await expect(caption).toBeVisible();
        await expect
          .poll(() =>
            caption.evaluate((element) => {
              const text = element.querySelector<HTMLElement>('.kui-text')!;
              const tail = element.querySelector<HTMLElement>('.kui-text__truncate-tail');
              if (!tail) return { tail: null };
              const box = text.getBoundingClientRect(),
                tailBox = tail.getBoundingClientRect();
              return {
                tail: tail.textContent,
                tailClipped: tail.scrollWidth > tail.clientWidth + 1,
                tailInside: tailBox.left >= box.left - 1 && tailBox.right <= box.right + 1,
              };
            }),
          )
          .toEqual({ tail: RUN.slice(-8), tailClipped: false, tailInside: true });
        await page.screenshot({ path: `target/visual-captures/hs2-kthjn2-drawer-${scheme}-${width}.png` });
      }
    }
  } finally {
    await app.stop();
  }
});

// HS2-G2X7RT: a session an orchestrator starts here whose tool reports its session start marks its
// drawer tab as waiting on a person until that report arrives, as it would while the tool sits on a
// first-run question in the terminal; the mark clears live when the tool reports in.
test('marks an orchestrator session waiting on its tool in the real drawer (HS2-G2X7RT)', async ({
  context,
  baseURL,
}) => {
  test.setTimeout(150_000);
  const app = await desktopApp(context, baseURL!, {
    canonicalRoot: true,
    seed: async (fixture) => syntheticTool(fixture.env.HOTSHEET_HOME!, { reportsStart: true }),
  });
  try {
    const started = await app.server.request<{
      outcome: string;
      session?: { tool_ready?: boolean; session: { session_id: string } };
    }>(`/checkouts/${app.server.checkoutId}/target-sessions`, 'POST', {
      session: {
        operation_id: 'sched-start-waiting',
        tool: TOOL,
        origin: { owner_checkout_id: app.server.checkoutId, project: 'Release train', run_id: RUN },
      },
      limits: { max_sessions: 64, min_free_ram_mb: 0, max_cpu_percent: 100, min_free_disk_mb: 0 },
      actor: person,
    });
    expect(started, JSON.stringify(started)).toMatchObject({ outcome: 'started', session: { tool_ready: false } });
    const terminal = started.session!.session.session_id;
    const listed = (await app.server.request<TerminalInfo[]>('/terminals')).find((row) => row.id === terminal);
    expect(listed?.tool_ready).toBe(false);

    const page = await app.launch();
    await page.setViewportSize({ width: 1440, height: 900 });
    await openMockedProject(page);
    await page.getByRole('button', { name: 'Show Terminal Drawer' }).click();
    const drawer = page.locator('[data-component="terminal-drawer"]');
    const tab = drawer.locator(`[data-tab-kind="terminal"][data-terminal-id="${terminal}"]`);
    const waiting = tab.locator('.terminal-drawer__tool-waiting[data-lucide="hand"]');
    await expect(waiting).toBeVisible();
    await expect(waiting.locator('title')).toHaveText('Waiting for an answer in this terminal');
    for (const scheme of ['light', 'dark'] as const) {
      await page.emulateMedia({ colorScheme: scheme });
      for (const width of [1440, 1100, 390, 1440]) {
        await page.setViewportSize({ width, height: 900 });
        await page.waitForFunction(() =>
          document.getAnimations().every((animation) => animation.playState !== 'running'),
        );
        await expect(waiting).toBeVisible();
        // The mark sits inside the tab, on the label's center line.
        const [tabBox, iconBox, nameBox] = await Promise.all([
          tab.boundingBox(),
          waiting.boundingBox(),
          tab.locator('.kui-tab-bar-item__name').boundingBox(),
        ]);
        expect(iconBox!.x).toBeGreaterThanOrEqual(tabBox!.x);
        expect(iconBox!.x + iconBox!.width).toBeLessThanOrEqual(tabBox!.x + tabBox!.width + 0.5);
        expect(Math.abs(nameBox!.y + nameBox!.height / 2 - (iconBox!.y + iconBox!.height / 2))).toBeLessThanOrEqual(
          1.5,
        );
        await page.screenshot({ path: `target/visual-captures/hs2-g2x7rt-drawer-${scheme}-${width}.png` });
      }
    }
    // A person answered and the tool's SessionStart hook reported in: the mark clears live.
    // The route answers 204 with no body, which the JSON helper cannot parse.
    await app.server
      .request(`/terminals/${terminal}/ai-connection`, 'POST', { agent: 'synthetic' })
      .catch((error: unknown) => {
        if (!(error instanceof SyntaxError)) throw error;
      });
    await expect(waiting).toHaveCount(0);
    await page.emulateMedia({ colorScheme: 'light' });
    await page.waitForFunction(() => document.getAnimations().every((animation) => animation.playState !== 'running'));
    await page.screenshot({ path: 'target/visual-captures/hs2-g2x7rt-drawer-ready-light-1440.png' });
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
    await openMockedProject(page);
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

// HS2-HVKS7M: closing an orchestrator-started session's drawer tab is a person's stop. The host
// records it as `person_stopped`, not as a crash the orchestrator would learn as `host_lost`.
test("records closing an orchestrator session's tab as a person's stop (HS2-HVKS7M)", async ({ context, baseURL }) => {
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
          operation_id: 'sched-start-close',
          tool: TOOL,
          origin: { owner_checkout_id: app.server.checkoutId, project: 'Release train', run_id: RUN },
        },
        limits: { max_sessions: 64, min_free_ram_mb: 0, max_cpu_percent: 100, min_free_disk_mb: 0 },
        actor: { role: 'human', id: 'maintainer' },
      },
    );
    expect(started, JSON.stringify(started)).toMatchObject({ outcome: 'started' });
    const terminal = started.session!.session.session_id;
    const page = await app.launch();
    await page.setViewportSize({ width: 1440, height: 900 });
    await openMockedProject(page);
    await page.getByRole('button', { name: 'Show Terminal Drawer' }).click();
    const tab = page
      .locator('[data-component="terminal-drawer"]')
      .locator(`[data-tab-kind="terminal"][data-terminal-id="${terminal}"]`);
    await expect(tab).toBeVisible();
    await tab.click({ button: 'right' });
    await page
      .locator('[data-context-menu="app-tab"]')
      .getByRole('menuitem', { name: 'Close Tab', exact: true })
      .click();
    await expect(tab).toHaveCount(0);
    const ended = async () =>
      (
        await app.server.request<{
          sessions: { session: { operation_id: string; ended_at_ms?: number; end_reason?: string } }[];
        }>(hosted)
      ).sessions.find((item) => item.session.operation_id === 'sched-start-close')?.session;
    await expect.poll(async () => (await ended())?.end_reason).toBe('person_stopped');
  } finally {
    await app.stop();
  }
});

// HS2-KR28FS: a session a person took over or paused can be handed back to its orchestrator from
// its drawer tab. Hand Back to Orchestrator is offered only while the session is the person's; it
// goes through the orchestrating checkout on this machine with the permission a session start
// needs (remembered here), and the orchestrator owns the session again, unpaused, so the tab offers
// Take Over again.
test('hands a taken-over session back to its orchestrator from its drawer tab (HS2-KR28FS)', async ({
  context,
  baseURL,
}) => {
  test.setTimeout(180_000);
  const app = await desktopApp(context, baseURL!, {
    canonicalRoot: true,
    seed: async (fixture) => syntheticTool(fixture.env.HOTSHEET_HOME!),
  });
  try {
    const server = app.server;
    await orchestrateSelf(server);
    const base = `/checkouts/${server.checkoutId}/orchestration`;
    await send(server, 'PUT', `${base}/policy`, {
      expected_revision: null,
      policy: {
        machine: { max_sessions: 64, min_free_ram_mb: 0, max_cpu_percent: 100, min_free_disk_mb: 0 },
        tools: [{ tool: TOOL, max_sessions: 4 }],
        targets: [
          {
            target_id: TARGET,
            actions: ['tickets_read', 'gates_observe', 'sessions_manage'],
            tools: [{ tool: TOOL }],
            max_sessions: 4,
          },
        ],
      },
      actor: person,
    });
    await send(server, 'POST', `${base}/permission-rules`, {
      target_id: TARGET,
      action: 'session_start',
      tool: TOOL,
      actor: person,
    });
    const started = (await send(server, 'POST', `${base}/sessions`, {
      target_id: TARGET,
      operation_id: 'sched-hand-back',
      tool: TOOL,
      actor: scheduler,
    })) as { session_id: string; state: string; hosted?: { session_id: string } };
    expect(started.state, JSON.stringify(started)).toBe('running');
    const terminal = started.hosted!.session_id;
    const record = async () =>
      (await send(server, 'GET', `${base}/sessions/${started.session_id}`)) as { owner: string; paused: boolean };
    const hosted = async () =>
      (await server.request<TerminalInfo[]>('/terminals')).find((row) => row.id === terminal)?.orchestrator;

    const page = await app.launch();
    await page.setViewportSize({ width: 1440, height: 900 });
    await openMockedProject(page);
    await page.getByRole('button', { name: 'Show Terminal Drawer' }).click();
    const tab = page
      .locator('[data-component="terminal-drawer"]')
      .locator(`[data-tab-kind="terminal"][data-terminal-id="${terminal}"]`);
    await expect(tab).toBeVisible();
    const menu = page.locator('[data-context-menu="app-tab"]');
    const openMenu = async () => {
      if (await menu.count()) await page.keyboard.press('Escape');
      await expect(menu).toHaveCount(0);
      await tab.click({ button: 'right' });
      await expect(menu.getByRole('menuitem', { name: 'Rename…' })).toBeVisible();
    };
    // The orchestrator's session: no hand-back is offered.
    await openMenu();
    await expect(menu.getByRole('menuitem', { name: 'Hand Back to Orchestrator' })).toHaveCount(0);
    await menu.getByRole('menuitem', { name: 'Take Over and Pause' }).click();
    await expect.poll(hosted).toMatchObject({ owner: 'human', paused: true });

    // The person's session offers Hand Back to Orchestrator, at every width.
    for (const width of [1440, 1100, 390]) {
      await page.setViewportSize({ width, height: 900 });
      await page.waitForFunction(() =>
        document.getAnimations().every((animation) => animation.playState !== 'running'),
      );
      await expect(async () => {
        await openMenu();
        await expect(menu.getByRole('menuitem', { name: 'Hand Back to Orchestrator' })).toBeVisible({
          timeout: 1_000,
        });
      }).toPass();
      await expect(menu.getByRole('menuitem', { name: 'Hand Back to Orchestrator' })).toBeInViewport({ ratio: 1 });
      await expect(menu.getByRole('menuitem', { name: /Take Over/u })).toHaveCount(0);
      await page.waitForFunction(() =>
        document.getAnimations().every((animation) => animation.playState !== 'running'),
      );
      await page.screenshot({ path: `target/visual-captures/hs2-kr28fs-menu-${width}.png` });
      await page.keyboard.press('Escape');
    }

    // The same menu in the dark scheme.
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.emulateMedia({ colorScheme: 'dark' });
    await openMenu();
    await expect(menu.getByRole('menuitem', { name: 'Hand Back to Orchestrator' })).toBeVisible();
    await page.waitForFunction(() => document.getAnimations().every((animation) => animation.playState !== 'running'));
    await page.screenshot({ path: `target/visual-captures/hs2-kr28fs-menu-dark-1440.png` });
    await page.keyboard.press('Escape');
    await page.emulateMedia({ colorScheme: 'light' });

    // Handing it back: the orchestrator owns it again, unpaused, here and in its record.
    await openMenu();
    await menu.getByRole('menuitem', { name: 'Hand Back to Orchestrator' }).click();
    await expect(page.getByRole('status').filter({ hasText: 'Handed back to the orchestrator.' })).toHaveCount(1);
    await expect.poll(hosted).toMatchObject({ owner: 'orchestrator', paused: false });
    expect(await record()).toMatchObject({ owner: 'orchestrator', paused: false });
    await page.waitForFunction(() => document.getAnimations().every((animation) => animation.playState !== 'running'));
    await page.screenshot({ path: `target/visual-captures/hs2-kr28fs-handed-back-1440.png` });
    // The tab offers Take Over again, and no hand-back.
    await expect(async () => {
      await openMenu();
      await expect(menu.getByRole('menuitem', { name: 'Take Over from Orchestrator' })).toBeVisible({
        timeout: 1_000,
      });
    }).toPass();
    await expect(menu.getByRole('menuitem', { name: 'Hand Back to Orchestrator' })).toHaveCount(0);
    await page.keyboard.press('Escape');
  } finally {
    await app.stop();
  }
});

// HS2-BPTQ5X: a hand-back that waits on its session_start permission request finishes by itself once
// a person allows the request in the ordinary permission prompt, without choosing Hand Back again;
// a denial is reported in the drawer and leaves the session the person's.
test('finishes a hand-back once its permission is allowed and reports a denial (HS2-BPTQ5X)', async ({
  context,
  baseURL,
}) => {
  test.setTimeout(180_000);
  const app = await desktopApp(context, baseURL!, {
    canonicalRoot: true,
    seed: async (fixture) => syntheticTool(fixture.env.HOTSHEET_HOME!),
  });
  try {
    const server = app.server;
    await orchestrateSelf(server);
    const base = `/checkouts/${server.checkoutId}/orchestration`;
    await send(server, 'PUT', `${base}/policy`, {
      expected_revision: null,
      policy: {
        machine: { max_sessions: 64, min_free_ram_mb: 0, max_cpu_percent: 100, min_free_disk_mb: 0 },
        tools: [{ tool: TOOL, max_sessions: 4 }],
        targets: [
          {
            target_id: TARGET,
            actions: ['tickets_read', 'gates_observe', 'sessions_manage'],
            tools: [{ tool: TOOL }],
            max_sessions: 4,
          },
        ],
      },
      actor: person,
    });
    // The start is allowed by a remembered rule, which is then forgotten: a hand-back asks.
    const rule = (await send(server, 'POST', `${base}/permission-rules`, {
      target_id: TARGET,
      action: 'session_start',
      tool: TOOL,
      actor: person,
    })) as { rule_id: string };
    const started = (await send(server, 'POST', `${base}/sessions`, {
      target_id: TARGET,
      operation_id: 'sched-hand-back-asks',
      tool: TOOL,
      actor: scheduler,
    })) as { session_id: string; state: string; hosted?: { session_id: string } };
    expect(started.state, JSON.stringify(started)).toBe('running');
    await send(server, 'DELETE', `${base}/permission-rules/${rule.rule_id}`, { actor: person });
    const terminal = started.hosted!.session_id;
    const hosted = async () =>
      (await server.request<TerminalInfo[]>('/terminals')).find((row) => row.id === terminal)?.orchestrator;

    const page = await app.launch();
    await page.setViewportSize({ width: 1440, height: 900 });
    await openMockedProject(page);
    await page.getByRole('button', { name: 'Show Terminal Drawer' }).click();
    const tab = page
      .locator('[data-component="terminal-drawer"]')
      .locator(`[data-tab-kind="terminal"][data-terminal-id="${terminal}"]`);
    await expect(tab).toBeVisible();
    const menu = page.locator('[data-context-menu="app-tab"]');
    const choose = async (item: string) => {
      await expect(async () => {
        if (await menu.count()) await page.keyboard.press('Escape');
        await expect(menu).toHaveCount(0);
        await tab.click({ button: 'right' });
        await expect(menu.getByRole('menuitem', { name: item })).toBeVisible({ timeout: 1_000 });
      }).toPass();
      await menu.getByRole('menuitem', { name: item }).click();
    };
    const toast = (text: string) => page.getByRole('status').filter({ hasText: text });
    const popup = page.locator('[data-component="permission-request-popup"]');

    await choose('Take Over from Orchestrator');
    await expect.poll(hosted).toMatchObject({ owner: 'human' });
    // The hand-back asks through the ordinary permission prompt and waits.
    await choose('Hand Back to Orchestrator');
    await expect(popup).toContainText('Orchestrator', { timeout: 30_000 });
    await expect(toast('It finishes once the request is allowed.')).toHaveCount(1);
    await page.waitForFunction(() => document.getAnimations().every((animation) => animation.playState !== 'running'));
    await page.screenshot({ path: `target/visual-captures/hs2-bptq5x-asks-1440.png` });
    expect(await hosted()).toMatchObject({ owner: 'human' });
    // Allowing it finishes the hand-back by itself.
    await popup.getByRole('button', { name: 'Allow Once' }).click();
    await expect(toast('Handed back to the orchestrator.')).toHaveCount(1, { timeout: 30_000 });
    await expect.poll(hosted).toMatchObject({ owner: 'orchestrator', paused: false });
    await page.waitForFunction(() => document.getAnimations().every((animation) => animation.playState !== 'running'));
    await page.screenshot({ path: `target/visual-captures/hs2-bptq5x-allowed-1440.png` });

    // Taken over again, a new hand-back asks again; Deny is reported and it stays the person's.
    await choose('Take Over from Orchestrator');
    await expect.poll(hosted).toMatchObject({ owner: 'human' });
    await choose('Hand Back to Orchestrator');
    await expect(popup).toContainText('Orchestrator', { timeout: 30_000 });
    await popup.getByRole('button', { name: 'Deny' }).click();
    await expect(toast('Permission to start a session was denied. The session stays yours.')).toHaveCount(1, {
      timeout: 30_000,
    });
    expect(await hosted()).toMatchObject({ owner: 'human' });
    await page.waitForFunction(() => document.getAnimations().every((animation) => animation.playState !== 'running'));
    await page.screenshot({ path: `target/visual-captures/hs2-bptq5x-denied-1440.png` });
  } finally {
    await app.stop();
  }
});
