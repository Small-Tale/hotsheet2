import { expect, test } from '@playwright/test';

import type { PermissionRequest, PollResponse, TerminalInfo } from '../src/api';
import { desktopApp } from './desktop-draft-files-fixture';
import { printThenEcho } from './terminal-fixture-command';

const TARGET = 'codex-synthetic-target';
const OTHER = 'codex-synthetic-other';

test('reconciles independent hook and MCP attribution without guessing terminal ownership (HS2-HBA9Z8)', async ({
  context,
  baseURL,
}) => {
  test.setTimeout(150_000);
  const app = await desktopApp(context, baseURL!);
  const pending = new Set<AbortController>();
  try {
    // Both terminals share a directory and tool. Neither is evidence for the other's ownership.
    for (const id of [TARGET, OTHER])
      await app.server.request('/terminals', 'POST', {
        id,
        connect: 'codex',
        ...printThenEcho(''),
        cwd: app.server.root,
      });
    const page = await app.launch();
    // Advance only the browser's local grace display. The isolated server and MCP TTL
    // retain real time; no network polling interval is accelerated by this simulation.
    await page.clock.install();
    await page.getByRole('button', { name: 'Open Project' }).click();
    await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();
    await page.getByRole('button', { name: 'Show Terminal Drawer' }).click();
    const drawer = page.locator('[data-component="terminal-drawer"]');
    const tab = (id: string) => drawer.locator(`[data-tab-kind="terminal"][data-terminal-id="${id}"]`);
    const icon = (id: string) => tab(id).locator('.terminal-drawer__ai-connection');
    const tile = (id: string) => drawer.locator(`.terminal-tile[data-terminal-key="${app.server.checkoutId}:${id}"]`);
    const tileIcon = (id: string) => tile(id).locator('.terminal-tile__ai-connection');
    await expect(tab(TARGET)).toBeVisible();
    await page.clock.fastForward(16_000);
    await page.clock.resume();

    const tabState = async (id: string, expected: 'missing' | 'mcp_missing' | 'connected') => {
      await expect(tab(id)).toHaveAttribute('data-ai-connection', expected);
      // HS2-YHQM67: hooks that route permission prompts keep a connected plug even without MCP; a
      // push-capable session may also be its project's primary (plug-zap).
      await expect(icon(id)).toHaveAttribute('data-lucide', expected === 'missing' ? 'unplug' : /^plug(-zap)?$/);
    };
    const state = async (id: string, expected: 'missing' | 'mcp_missing' | 'connected') => {
      await tabState(id, expected);
      await expect(drawer).toHaveAttribute('data-mode', 'grid');
      await expect(tileIcon(id)).toHaveAttribute('data-ai-connection', expected);
      await expect(
        tileIcon(id).locator(expected === 'missing' ? '[data-lucide="unplug"]' : '[data-lucide^="plug"]'),
      ).toHaveCount(1);
      await expect(tileIcon(id)).toHaveAttribute('title', (await icon(id).locator('title').textContent())!);
    };
    const snapshot = async () => {
      const rows = await app.server.request<TerminalInfo[]>('/terminals');
      expect(rows).toHaveLength(2);
      return { target: rows.find((row) => row.id === TARGET)!, other: rows.find((row) => row.id === OTHER)! };
    };
    const api = async (path: string, method: string, body?: unknown) => {
      const response = await fetch(`${app.server.url}${path}`, {
        method,
        headers: { 'content-type': 'application/json', 'X-Hotsheet-Secret': app.server.secret },
        ...(body === undefined ? {} : { body: JSON.stringify(body) }),
      });
      expect(response.ok, `${method} ${path}: ${response.status}`).toBe(true);
      return response;
    };
    const register = (terminal?: string) =>
      api('/mcp-presence', 'POST', {
        lease_id: 'synthetic-attribution-mcp',
        pid: process.pid,
        tool: 'codex',
        cwd: app.server.root,
        ...(terminal ? { terminal_id: terminal } : {}),
      });
    const cursor = async () => (await app.server.request<PollResponse>('/ws/poll?timeout_ms=0')).cursor;
    const events = async (since: number) =>
      (await app.server.request<PollResponse>(`/ws/poll?since=${since}&timeout_ms=0`)).events;
    const ask = (terminal?: string) => {
      const controller = new AbortController();
      pending.add(controller);
      const answer = fetch(`${app.server.url}/permissions/ask`, {
        method: 'POST',
        headers: { 'content-type': 'application/json', 'X-Hotsheet-Secret': app.server.secret },
        body: JSON.stringify({
          project: `checkout:${app.server.checkoutId}`,
          connection: 'synthetic-codex-attribution',
          agent: 'codex',
          tool: 'Bash',
          action: 'echo synthetic-attribution',
          // This non-UUID cannot name a daemon thread; the server rejects it before
          // Codex-home discovery (codex_watch.rs, HS2-HBA9Z8).
          session_id: 'synthetic-session',
          ...(terminal ? { terminal_id: terminal } : {}),
        }),
        signal: controller.signal,
      })
        .then(async (response) => {
          expect(response.ok).toBe(true);
          return response.json() as Promise<{ decision: string }>;
        })
        .catch((error: unknown) => {
          if (controller.signal.aborted) return { decision: 'withdrawn' };
          throw error;
        })
        .finally(() => pending.delete(controller));
      return answer;
    };
    const waitForAsk = async () => {
      await expect.poll(async () => (await app.server.request<PermissionRequest[]>('/permissions')).length).toBe(1);
      await expect(page.locator('[data-component="permission-request-popup"]')).toContainText('synthetic-attribution');
      return (await app.server.request<PermissionRequest[]>('/permissions'))[0].id;
    };
    const deny = async (id: number, answer: Promise<{ decision: string }>) => {
      await api(`/permissions/${id}`, 'POST', { decision: 'deny', scope: 'once' });
      expect(await answer).toEqual({ decision: 'deny' });
      await expect(page.locator('[data-component="permission-request-popup"]')).toHaveCount(0);
    };
    const capture = async (label: string) => {
      await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))));
      await page.waitForFunction(() =>
        document
          .getAnimations()
          .every(
            (animation) =>
              animation.playState !== 'running' || animation.effect?.getComputedTiming().endTime === Infinity,
          ),
      );
      await page.screenshot({ path: test.info().outputPath(`hs2-hba9z8-${label}.png`), animations: 'disabled' });
    };

    await state(TARGET, 'missing');
    await state(OTHER, 'missing');
    await expect(icon(TARGET).locator('title')).toContainText('No trusted hook report has reached this server.');
    await capture('hook-missing-wide');
    await tab(TARGET).screenshot({ path: test.info().outputPath('hs2-hba9z8-hook-missing-tab-wide.png') });

    // Permission delivery alone cannot attribute a missing or expired terminal environment.
    for (const attribution of [undefined, 'synthetic-expired-terminal']) {
      const before = await cursor();
      const answer = ask(attribution);
      const id = await waitForAsk();
      const current = await snapshot();
      expect(current.target.ai_connection).toBeUndefined();
      expect(current.other.ai_connection).toBeUndefined();
      expect(current.target.last_hook_report).toBeUndefined();
      expect(current.other.last_hook_report).toBeUndefined();
      const emitted = await events(before);
      expect(emitted.some((event) => event.kind === 'permission_asked')).toBe(true);
      expect(emitted.some((event) => ['terminal_hook_report', 'terminal_ai_connection'].includes(event.kind))).toBe(
        false,
      );
      await state(TARGET, 'missing');
      await state(OTHER, 'missing');
      await deny(id, answer);
    }

    // A correctly attributed trusted report changes the target before its permission is answered.
    const before = await cursor();
    const answer = ask(TARGET);
    const id = await waitForAsk();
    const current = await snapshot();
    expect(current.target.ai_connection).toMatchObject({ agent: 'codex', source: 'permission_request' });
    expect(current.target.last_hook_report).toMatchObject({ source: 'permission_request' });
    expect(current.target.mcp_connected).toBe(false);
    expect(current.other.ai_connection).toBeUndefined();
    expect(
      (await events(before))
        .filter((event) => ['terminal_hook_report', 'terminal_ai_connection', 'permission_asked'].includes(event.kind))
        .map((event) => event.kind),
    ).toEqual(['terminal_hook_report', 'terminal_ai_connection', 'permission_asked']);
    await state(TARGET, 'mcp_missing');
    await state(OTHER, 'missing');
    await expect(icon(TARGET).locator('title')).toContainText('Its permission prompts come to the app');
    await expect(icon(TARGET).locator('title')).toContainText('MCP not connected');
    await expect(icon(TARGET).locator('title')).toContainText('Last trusted hook report: PermissionRequest');
    // Dismiss the popup presentation without resolving the still-pending request, to inspect the tab.
    await page
      .locator('[data-component="permission-request-popup"]')
      .getByRole('button', { name: 'Ignore', exact: true })
      .click();
    await expect(page.locator('[data-component="permission-request-popup"]')).toHaveCount(0);
    await capture('hook-only-mcp-missing-wide');

    for (const attribution of [undefined, OTHER, TARGET]) {
      await register(attribution);
      expect(await app.server.request<Record<string, unknown>>('/mcp-presence')).toMatchObject({ connected: true });
      const current = await snapshot();
      expect(current.target.mcp_connected).toBe(attribution === TARGET);
      expect(current.other.mcp_connected).toBe(attribution === OTHER);
      expect(current.target.ai_connection?.source).toBe('permission_request');
      expect(current.other.ai_connection).toBeUndefined();
      await state(TARGET, attribution === TARGET ? 'connected' : 'mcp_missing');
      await state(OTHER, 'missing');
      expect(await app.server.request<PermissionRequest[]>('/permissions')).toHaveLength(1);
    }
    await expect(icon(TARGET).locator('title')).toContainText('its hotsheet-mcp tools are connected');
    for (const [label, width] of [
      ['wide', 1280],
      ['medium', 900],
      ['phone', 390],
      ['wide-return', 1280],
    ] as const) {
      await register(TARGET);
      await page.setViewportSize({ width, height: 844 });
      await state(TARGET, 'connected');
      await capture(`connected-grid-${label}`);
      await tab(TARGET).getByRole('tab').click();
      await expect(drawer).toHaveAttribute('data-mode', 'dedicated');
      await tabState(TARGET, 'connected');
      await expect(tile(TARGET)).toHaveCount(0);
      await capture(`connected-tab-${label}`);
      await drawer.getByRole('button', { name: 'Project Grid', exact: true }).click();
      await state(TARGET, 'connected');
    }
    await tab(TARGET).screenshot({
      path: test.info().outputPath('hs2-hba9z8-connected-tab-detail-wide.png'),
      animations: 'disabled',
    });
    await deny(id, answer);

    // Reset and repeat in both directions: MCP loss leaves the trusted hook, hook loss leaves MCP.
    await api('/mcp-presence/synthetic-attribution-mcp', 'DELETE');
    await state(TARGET, 'mcp_missing');
    await register(TARGET);
    await state(TARGET, 'connected');
    await api(`/terminals/${TARGET}/ai-connection`, 'DELETE');
    await state(TARGET, 'missing');
    await expect(icon(TARGET).locator('title')).toContainText('Its hotsheet-mcp tools are connected, but');
    expect((await snapshot()).target.last_hook_report?.source).toBe('permission_request');
    const again = ask(TARGET);
    const againId = await waitForAsk();
    await state(TARGET, 'connected');
    await deny(againId, again);
  } finally {
    for (const controller of pending) controller.abort();
    await app.stop();
  }
});

test('keeps every hooked session connected and marks one primary across close and reopen (HS2-YHQM67)', async ({
  context,
  baseURL,
}) => {
  test.setTimeout(150_000);
  const app = await desktopApp(context, baseURL!);
  try {
    // Synthetic Claude sessions only: each reports a hook-connected session with an inbox that
    // Hot Sheet could push to, which makes it eligible to be the project's primary.
    const open = async (id: string) => {
      await app.server.request('/terminals', 'POST', {
        id,
        connect: 'claude',
        command: '/bin/sh',
        args: ['-c', 'exec cat'],
        cwd: app.server.root,
      });
      await app.server.request('/mcp-presence', 'POST', {
        lease_id: `synthetic-mcp-${id}`,
        pid: process.pid,
        tool: 'claude',
        cwd: app.server.root,
        terminal_id: id,
      });
      // The hook report answers 204 with no body.
      const report = await fetch(`${app.server.url}/terminals/${id}/ai-connection`, {
        method: 'POST',
        headers: { 'content-type': 'application/json', 'X-Hotsheet-Secret': app.server.secret },
        body: JSON.stringify({
          agent: 'claude',
          session_id: `synthetic-session-${id}`,
          messaging_socket: `${app.server.root}/synthetic-${id}.sock`,
          messaging_token: 'synthetic-inbox-token',
          tool_version: '2.1.302',
        }),
      });
      expect(report.status).toBe(204);
    };
    await open('primary-a');
    await open('primary-b');
    const page = await app.launch();
    await page.getByRole('button', { name: 'Open Project' }).click();
    await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();
    await page.getByRole('button', { name: 'Show Terminal Drawer' }).click();
    const drawer = page.locator('[data-component="terminal-drawer"]');
    const tab = (id: string) => drawer.locator(`[data-tab-kind="terminal"][data-terminal-id="${id}"]`);
    const icon = (id: string) => tab(id).locator('.terminal-drawer__ai-connection');
    const expectTabs = async (primary: string, secondary: string[]) => {
      await expect(tab(primary)).toHaveAttribute('data-ai-connection', 'connected');
      await expect(tab(primary)).toHaveAttribute('data-ai-primary', 'true');
      await expect(icon(primary)).toHaveAttribute('data-lucide', 'plug-zap');
      await expect(icon(primary).locator('title')).toContainText('Primary session');
      for (const id of secondary) {
        // A connected secondary session is never drawn as disconnected.
        await expect(tab(id)).toHaveAttribute('data-ai-connection', 'connected');
        await expect(tab(id)).not.toHaveAttribute('data-ai-primary', 'true');
        await expect(icon(id)).toHaveAttribute('data-lucide', 'plug');
        await expect(icon(id).locator('title')).not.toContainText('Primary session');
      }
    };
    const primaries = async () =>
      (await app.server.request<TerminalInfo[]>('/terminals'))
        .filter((row) => row.feedback_primary)
        .map((row) => row.id);
    await expectTabs('primary-a', ['primary-b']);
    expect(await primaries()).toEqual(['primary-a']);
    for (const [label, width] of [
      ['wide', 1280],
      ['medium', 900],
      ['narrow', 390],
      ['wide-return', 1280],
    ] as const) {
      await page.setViewportSize({ width, height: 844 });
      await expectTabs('primary-a', ['primary-b']);
      await page.screenshot({ path: test.info().outputPath(`hs2-yhqm67-two-sessions-${label}.png`) });
    }
    await tab('primary-a').screenshot({ path: test.info().outputPath('hs2-yhqm67-primary-tab-detail.png') });

    // Close the primary from its tab: the remaining session is promoted without a reload.
    await tab('primary-a').hover();
    await tab('primary-a').getByRole('button', { name: /close/i }).click();
    await expect(tab('primary-a')).toHaveCount(0);
    await expectTabs('primary-b', []);
    expect(await primaries()).toEqual(['primary-b']);
    await page.screenshot({ path: test.info().outputPath('hs2-yhqm67-promoted-wide.png') });

    // Reopen a terminal: it is connected, and the promoted primary keeps the role.
    await open('primary-c');
    await expect(tab('primary-c')).toBeVisible();
    await expectTabs('primary-b', ['primary-c']);
    expect(await primaries()).toEqual(['primary-b']);
    await page.screenshot({ path: test.info().outputPath('hs2-yhqm67-reopened-wide.png') });
  } finally {
    await app.stop();
  }
});
