import { once } from 'node:events';
import { mkdtemp, readdir, readFile, rm } from 'node:fs/promises';
import type { Server } from 'node:http';
import type { AddressInfo } from 'node:net';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { chromium, type Page } from '@playwright/test';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import WebSocket from 'ws';

import { execFileAsync } from './child-process';
import { createDevApp } from './dev-server';
import { createLocalHostApp, startLocalHost } from './local-host';
import { createLocalGitTicketStore, openLocalProject } from './project-bridge';

// Built CLI/server binaries run against a private, synthetic machine home and store.
// Run with HOTSHEET_LIVE_SERVER=1 after building both binaries from this checkout.
const live = process.env.HOTSHEET_LIVE_SERVER === '1';
const cli = fileURLToPath(new URL('../../../target/debug/hotsheet-cli', import.meta.url));

async function waitForExit(pid: number): Promise<void> {
  for (let n = 0; n < 300; n += 1) {
    try {
      process.kill(pid, 0);
    } catch {
      return;
    }
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
}

describe.skipIf(!live)('HS2-7DBJMS real browser bridge and server', () => {
  let home = '';
  let workspace = '';
  let store = '';
  let project = '';
  let checkoutId = '';
  let checkoutRoot = '';
  let origin = '';
  let host: Server | undefined;
  let previousHome: string | undefined;
  let previousMode: string | undefined;

  beforeAll(async () => {
    previousHome = process.env.HOTSHEET_HOME;
    previousMode = process.env.HOTSHEET_BROWSER_AUTH_MODE;
    home = await mkdtemp(join(tmpdir(), 'hs2-remote-auth-home-'));
    workspace = await mkdtemp(join(tmpdir(), 'hs2-remote-auth-work-'));
    process.env.HOTSHEET_HOME = home;
    project = await mkdtemp(join(workspace, 'synthetic-project-'));
    store = await createLocalGitTicketStore(project);
    const opened = await openLocalProject(project, store);
    checkoutId = opened.id;
    checkoutRoot = opened.root;
    process.env.HOTSHEET_BROWSER_AUTH_MODE = 'remote';
    host = await startLocalHost({ port: 0, distRoot: fileURLToPath(new URL('../dist', import.meta.url)) });
    origin = `http://127.0.0.1:${(host.address() as AddressInfo).port}`;
  }, 120_000);

  afterAll(async () => {
    if (host)
      await new Promise<void>((resolve) => {
        host!.close(() => {
          resolve();
        });
      });
    const stopping: number[] = [];
    for (const name of await readdir(join(home, 'instances')).catch(() => [] as string[])) {
      if (!name.endsWith('.json')) continue;
      const info = JSON.parse(await readFile(join(home, 'instances', name), 'utf8').catch(() => '{}')) as {
        pid?: number;
      };
      if (typeof info.pid === 'number') {
        try {
          process.kill(info.pid, 'SIGTERM');
          stopping.push(info.pid);
        } catch {
          /* exited */
        }
      }
    }
    await Promise.all(stopping.map(waitForExit));
    if (previousHome === undefined) delete process.env.HOTSHEET_HOME;
    else process.env.HOTSHEET_HOME = previousHome;
    if (previousMode === undefined) delete process.env.HOTSHEET_BROWSER_AUTH_MODE;
    else process.env.HOTSHEET_BROWSER_AUTH_MODE = previousMode;
    await rm(home, { recursive: true, force: true, maxRetries: 5 });
    await rm(workspace, { recursive: true, force: true, maxRetries: 5 });
  }, 120_000);

  it('requires host approval, forwards only the session, and revokes both host paths', async () => {
    const bridge = createDevApp();
    const url = (path: string) => `${origin}/__hotsheet${path}`;
    const post = (path: string, value: unknown, headers: Record<string, string> = {}) =>
      bridge.request(url(path), {
        method: 'POST',
        headers: { origin, 'content-type': 'application/json', ...headers },
        body: JSON.stringify(value),
      });
    expect((await bridge.request(url('/checkouts'))).status).toBe(401);
    expect(
      (
        await bridge.request(url('/project-api/spoof/tickets'), {
          headers: { 'x-hotsheet-secret': 'synthetic-spoof' },
        })
      ).status,
    ).toBe(401);

    const pairing = await post('/remote-auth/pair', { label: 'Synthetic browser' });
    expect(pairing.status).toBe(201);
    const { id, challenge } = (await pairing.json()) as { id: string; challenge: string };
    expect((await post('/remote-auth/redeem', { id, challenge })).status).toBe(409);

    await execFileAsync(cli, ['-C', store, 'remote', 'sessions', 'approve', id, '--role', 'read_only'], {
      env: { ...process.env, HOTSHEET_HOME: home },
    });
    const redeemed = await post('/remote-auth/redeem', { id, challenge });
    expect(redeemed.status).toBe(200);
    const cookies = redeemed.headers.getSetCookie();
    expect(cookies).toHaveLength(2);
    const cookie = cookies.map((line) => line.split(';')[0]).join('; ');
    const csrf = cookies[1].split('=')[1].split(';')[0];
    const device = (await redeemed.json()) as { id: string; role: string };
    expect(device.role).toBe('read_only');
    expect((await post('/remote-auth/redeem', { id, challenge })).status).toBe(401);

    const headers = { cookie, origin };
    expect((await bridge.request(url('/checkouts'), { headers })).status).toBe(200);
    expect((await bridge.request(url(`/project-api/${checkoutId}/tickets`), { headers })).status).toBe(200);
    const localHost = createLocalHostApp('/nonexistent-synthetic-dist', createDevApp());
    expect((await localHost.request(url('/checkouts'), { headers })).status).toBe(200);
    expect((await localHost.request(url('/checkouts'))).status).toBe(401);
    expect((await bridge.request(url('/desktop-storage'), { headers })).status).toBe(404);
    expect(
      (
        await bridge.request(url('/project-api/spoof/tickets'), {
          headers: { ...headers, 'x-hotsheet-secret': 'synthetic-spoof' },
        })
      ).status,
    ).toBe(404); // session authenticated, project absent
    expect(
      (
        await bridge.request(url('/checkouts'), {
          headers: { cookie, origin: 'http://foreign.invalid' },
        })
      ).status,
    ).toBe(401);
    // HS2-3HC9HT: the remote session opens only registered checkouts, through its own route.
    const sessionPost = (path: string, value: unknown, from = origin) =>
      bridge.request(url(path), {
        method: 'POST',
        headers: { cookie, origin: from, 'content-type': 'application/json' },
        body: JSON.stringify(value),
      });
    expect((await sessionPost('/projects/open', { root: project })).status).toBe(404);
    expect((await sessionPost('/remote/projects/open', { root: workspace })).status).toBe(403);
    expect((await sessionPost('/remote/projects/open', { root: project }, 'http://foreign.invalid')).status).toBe(401);
    const remoteOpen = await sessionPost('/remote/projects/open', { root: project });
    expect(remoteOpen.status).toBe(201);
    expect(((await remoteOpen.json()) as { id: string }).id).toBe(checkoutId);
    // HS2-CD8YNK: behind Tailscale Serve the host sees a different request URL than the paired origin.
    const proxied = await bridge.request('http://synthetic-proxy.invalid/__hotsheet/remote/projects/open', {
      method: 'POST',
      headers: { cookie, origin, 'content-type': 'application/json' },
      body: JSON.stringify({ root: project }),
    });
    expect(proxied.status).toBe(201);
    // HS2-ZVSMTZ: the multiplexed change poll rides the remote session to the real server.
    const muxed = await bridge.request(url('/change-mux/poll'), {
      method: 'POST',
      headers: { cookie, origin, 'content-type': 'application/json' },
      body: JSON.stringify({ version: 1, client: 'synthetic-tab', polls: [{ project: checkoutId, timeout_ms: 0 }] }),
    });
    expect(muxed.status).toBe(200);
    expect(((await muxed.json()) as { results: Array<{ status: number }> }).results[0].status).toBe(200);
    // HS2-9FWETT: migration status for a registered checkout, refused for anything else.
    const status = (root: string) =>
      bridge.request(url(`/remote/projects/migration-jobs?root=${encodeURIComponent(root)}`), { headers });
    expect((await status(checkoutRoot)).status).toBe(200);
    expect((await status(workspace)).status).toBe(403);
    const deniedWrite = await bridge.request(url(`/project-api/${checkoutId}/tickets`), {
      method: 'POST',
      headers: { ...headers, 'x-hotsheet-csrf': csrf },
      body: '{}',
    });
    expect(deniedWrite.status).toBe(403);

    const socket = new WebSocket(`${origin.replace('http:', 'ws:')}/__hotsheet/project-api/${checkoutId}/ws/sync`, {
      headers: { cookie, origin },
    });
    await once(socket, 'open');

    await execFileAsync(cli, ['-C', store, 'remote', 'sessions', 'revoke', device.id], {
      env: { ...process.env, HOTSHEET_HOME: home },
    });
    await Promise.race([
      once(socket, 'close'),
      new Promise((_, reject) => {
        setTimeout(() => {
          reject(new Error('revoked WebSocket stayed open'));
        }, 3_000);
      }),
    ]);
    expect((await bridge.request(url('/checkouts'), { headers })).status).toBe(401);
    expect((await localHost.request(url('/checkouts'), { headers })).status).toBe(401);
  }, 120_000);

  it('renders and completes pairing in the shipped browser app at three widths', async () => {
    const browser = await chromium.launch({ headless: true });
    try {
      const page = await browser.newPage({ viewport: { width: 1280, height: 800 } });
      const errors: string[] = [];
      page.on('pageerror', (error) => errors.push(error.message));
      page.on('requestfailed', (request) => errors.push(`${request.url()}: ${request.failure()?.errorText}`));
      await page.goto(origin);
      await page.getByRole('heading', { name: 'Pair this browser with Hot Sheet' }).waitFor();
      await page.screenshot({ path: '/private/tmp/hs2-7dbjms-pairing-wide.png', fullPage: true });
      await page.setViewportSize({ width: 768, height: 900 });
      await page.screenshot({ path: '/private/tmp/hs2-7dbjms-pairing-medium.png', fullPage: true });
      await page.setViewportSize({ width: 375, height: 812 });
      await page.screenshot({ path: '/private/tmp/hs2-7dbjms-pairing-narrow.png', fullPage: true });
      await page.setViewportSize({ width: 1280, height: 800 });
      await page.getByRole('textbox', { name: 'Device name' }).fill('Synthetic browser UI');
      await page.getByRole('button', { name: 'Request Access' }).click();
      const id = await page.locator('.remote-auth-screen__request code').first().textContent();
      expect(id).toBeTruthy();
      await execFileAsync(cli, ['-C', store, 'remote', 'sessions', 'approve', id!, '--role', 'read_write'], {
        env: { ...process.env, HOTSHEET_HOME: home },
      });
      await page.getByRole('button', { name: 'Continue After Approval' }).click();
      await page.waitForTimeout(1_000);
      await page.screenshot({ path: '/private/tmp/hs2-7dbjms-after-approval.png', fullPage: true });
      await page
        .getByText('Open a Hot Sheet Project')
        .waitFor({ timeout: 10_000 })
        .catch(async () => {
          throw new Error(
            `Post-pairing app failed: ${errors.join(' | ')}; body=${(await page.locator('body').innerText()).slice(0, 300)}`,
          );
        });
      // HS2-3HC9HT: the approved session opens a registered checkout from the remote picker.
      const opens: number[] = [];
      page.on('response', (response) => {
        if (new URL(response.url()).pathname.endsWith('/projects/open')) opens.push(response.status());
      });
      await page.getByRole('button', { name: 'Open Project' }).click();
      await page.locator(`[data-checkout-root="${checkoutRoot}"]`).click();
      await page.getByText('Open a Hot Sheet Project').waitFor({ state: 'detached', timeout: 20_000 });
      expect(opens).toEqual([201]);
      await page.screenshot({ path: '/private/tmp/hs2-3hc9ht-remote-project-open.png', fullPage: true });
    } finally {
      await browser.close();
    }
  }, 120_000);

  /** Sessions the grant tests share, so the suite stays under the ten-requests-a-minute pairing limit. */
  const granted: Record<'plain' | 'admin' | 'host', { cookie: string; session: string; csrf: string; id: string }> =
    {} as never;

  it('HS2-85W6BE / HS2-DXM0YN trusts host actions and device management by session grant, not URL', async () => {
    const bridge = createDevApp();
    const url = (path: string) => `${origin}/__hotsheet${path}`;
    const cliRun = (args: string[]) =>
      execFileAsync(cli, ['-C', store, 'remote', 'sessions', ...args], {
        env: { ...process.env, HOTSHEET_HOME: home },
      });
    const request = (path: string, method = 'GET', body?: unknown, headers: Record<string, string> = {}) =>
      bridge.request(url(path), {
        method,
        headers: { origin, ...(body === undefined ? {} : { 'content-type': 'application/json' }), ...headers },
        ...(body === undefined ? {} : { body: JSON.stringify(body) }),
      });
    const pending = async (label: string) =>
      (await (await request('/remote-auth/pair', 'POST', { label })).json()) as { id: string; challenge: string };
    const pair = async (label: string, approve: string[]) => {
      const { id, challenge } = await pending(label);
      await cliRun(['approve', id, ...approve]);
      const redeemed = await request('/remote-auth/redeem', 'POST', { id, challenge });
      expect(redeemed.status).toBe(200);
      const cookies = redeemed.headers.getSetCookie().map((line) => line.split(';')[0]);
      const device = (await redeemed.json()) as { id: string; admin: boolean; local: boolean };
      return {
        cookie: cookies.join('; '),
        session: cookies[0].split('=')[1],
        csrf: cookies[1].split('=')[1],
        id: device.id,
        device,
      };
    };
    const plain = await pair('Synthetic phone', ['--role', 'read_write']);
    const admin = await pair('Synthetic tablet', ['--role', 'read_only', '--admin']);
    const host = await pair('Synthetic Mac', ['--role', 'read_write', '--admin', '--local']);
    expect([plain.device, admin.device, host.device].map(({ admin: a, local: l }) => [a, l])).toEqual([
      [false, false],
      [true, false],
      [true, true],
    ]);
    granted.plain = plain;
    granted.admin = admin;
    granted.host = host;
    const as = (who: { cookie: string }) => ({ cookie: who.cookie });

    // HS2-85W6BE: the same loopback origin is remote until the session carries the local grant.
    const latency = (who: { cookie: string }) => request('/dev/latency', 'GET', undefined, as(who));
    expect((await latency(plain)).status).toBe(404);
    expect((await latency(admin)).status).toBe(404);
    expect((await latency(host)).status).toBe(200);
    const openByPath = (who: { cookie: string }) => request('/projects/open', 'POST', { root: project }, as(who));
    expect((await openByPath(plain)).status).toBe(404);
    const opened = await openByPath(host);
    expect(opened.status).toBe(201);
    expect(((await opened.json()) as { id: string }).id).toBe(checkoutId);

    // HS2-DXM0YN: device management needs the admin grant; host access needs the local grant.
    expect((await request('/remote-auth/owner', 'GET', undefined, as(plain))).status).toBe(403);
    const listed = await request('/remote-auth/owner', 'GET', undefined, as(admin));
    expect(listed.status).toBe(200);
    const list = (await listed.json()) as {
      revision: number;
      manager: { device_id: string; local: boolean };
      pair_ttl_seconds: number;
    };
    expect(list.manager).toEqual({ device_id: admin.id, local: false });
    expect(list.pair_ttl_seconds).toBe(900);
    const waiting = request(`/remote-auth/owner/changes?after=${String(list.revision)}`, 'GET', undefined, as(admin));
    const target = await pending('Synthetic laptop');
    const woke = (await (await waiting).json()) as { pending: { id: string }[] };
    expect(woke.pending.map((entry) => entry.id)).toContain(target.id);
    expect(
      (
        await request(
          '/remote-auth/owner/approve',
          'POST',
          { id: target.id, role: 'read_only', local: true },
          as(admin),
        )
      ).status,
    ).toBe(403);
    expect(
      (await request('/remote-auth/owner/approve', 'POST', { id: target.id, role: 'read_only' }, as(admin))).status,
    ).toBe(204);
    expect((await request(`/remote-auth/owner/devices/${plain.id}`, 'PATCH', { local: true }, as(admin))).status).toBe(
      403,
    );
    const promoted = await request(`/remote-auth/owner/devices/${plain.id}`, 'PATCH', { admin: true }, as(host));
    expect(promoted.status).toBe(200);
    expect(((await promoted.json()) as { admin: boolean }).admin).toBe(true);
    // Headless parity: the CLI rejects a request and changes grants through the same workflow.
    const stranger = await pending('Synthetic stranger');
    await cliRun(['reject', stranger.id]);
    expect((await request('/remote-auth/redeem', 'POST', stranger)).status).toBe(401);
    await cliRun(['update', plain.id, '--admin', 'false']);
    expect((await request('/remote-auth/owner', 'GET', undefined, as(plain))).status).toBe(403);
    const status = JSON.parse((await cliRun(['status'])).stdout) as { devices: { id: string; local: boolean }[] };
    expect(status.devices.find((device) => device.id === host.id)?.local).toBe(true);
  }, 120_000);

  it('HS2-DXM0YN shows App Settings → Devices to a local admin and explains it to a plain session', async () => {
    expect(granted.host).toBeDefined();
    const browser = await chromium.launch({ headless: true });
    const captures = process.env.HOTSHEET_CAPTURE_DIR ?? tmpdir();
    const capture = (name: string) => join(captures, `hs2-dxm0yn-${name}.png`);
    // Rails slide when the width changes; capture once every animation has finished.
    const settled = (target: Page) =>
      target.waitForFunction(() =>
        document
          .getAnimations()
          .every(
            (animation) =>
              animation.effect?.getComputedTiming().iterations === Infinity || animation.playState !== 'running',
          ),
      );
    const signedIn = async (who: { session: string; csrf: string }) => {
      const context = await browser.newContext({ viewport: { width: 1280, height: 800 } });
      await context.addCookies([
        { name: 'hs2_remote_session', value: who.session, url: origin, httpOnly: true, sameSite: 'Strict' },
        { name: 'hs2_remote_csrf', value: who.csrf, url: origin, sameSite: 'Strict' },
      ]);
      return context.newPage();
    };
    try {
      // HS2-85W6BE: the local admin at 127.0.0.1 gets the local open flow and an enabled folder chooser.
      const page = await signedIn(granted.host);
      await page.goto(origin);
      await page.getByRole('button', { name: 'Open Project' }).click();
      await page.getByRole('button', { name: 'Local Project' }).click();
      const browse = page.getByRole('button', { name: 'Browse for Project Folder' });
      await expect(browse.isEnabled()).resolves.toBe(true);
      await page.getByRole('textbox', { name: 'Project Folder' }).fill(project);
      await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();
      await page.getByText('Open a Hot Sheet Project').waitFor({ state: 'detached', timeout: 20_000 });
      await page.getByLabel('Settings View').click();
      await page.locator('nav[aria-label="Settings categories"] [data-item-id="devices"]').first().click();
      const devices = page.locator('[data-component="remote-devices-settings"]');
      await devices.getByText('Synthetic tablet', { exact: true }).waitFor();
      await expect(devices.getByText('This Device', { exact: true }).count()).resolves.toBe(1);
      await settled(page);
      await page.screenshot({ path: capture('devices-wide'), fullPage: true });
      // The row's menu holds its commands; Host Access is enabled for a host-trusted caller.
      const tablet = devices.locator('[data-remote-device]').filter({ hasText: 'Synthetic tablet' });
      await tablet.getByRole('button').first().click();
      const hostAccess = tablet.locator('[data-action="toggle-remote-device-host-access"]');
      await hostAccess.waitFor({ state: 'visible' });
      await expect(hostAccess.isDisabled()).resolves.toBe(false);
      await settled(page);
      await page.screenshot({ path: capture('devices-menu-wide'), fullPage: true });
      await page.keyboard.press('Escape');
      await page.setViewportSize({ width: 768, height: 900 });
      await settled(page);
      await page.screenshot({ path: capture('devices-medium'), fullPage: true });
      await page.setViewportSize({ width: 375, height: 812 });
      await settled(page);
      await page.screenshot({ path: capture('devices-narrow'), fullPage: true });
      await page.setViewportSize({ width: 1280, height: 800 });
      await devices.getByText('Synthetic tablet', { exact: true }).waitFor();
      await settled(page);
      await page.screenshot({ path: capture('devices-wide-again'), fullPage: true });

      // A session without the admin grant sees the category with the reason, not the list.
      const other = await signedIn(granted.plain);
      await other.goto(origin);
      await other.getByRole('button', { name: 'Open Project' }).click();
      // Without the local grant the loopback page keeps the registered-project picker.
      await other.locator(`[data-checkout-root="${checkoutRoot}"]`).click();
      await other.getByText('Open a Hot Sheet Project').waitFor({ state: 'detached', timeout: 20_000 });
      await other.getByLabel('Settings View').click();
      await other.locator('nav[aria-label="Settings categories"] [data-item-id="devices"]').first().click();
      await other.getByText('Administrator access required.').waitFor();
      await settled(other);
      await other.screenshot({ path: capture('admin-required-wide'), fullPage: true });
      await other.setViewportSize({ width: 375, height: 812 });
      await settled(other);
      await other.screenshot({ path: capture('admin-required-narrow'), fullPage: true });
    } finally {
      await browser.close();
    }
  }, 180_000);
});
