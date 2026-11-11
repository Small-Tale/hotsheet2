import { once } from 'node:events';
import { mkdtemp, readdir, readFile, rm } from 'node:fs/promises';
import type { Server } from 'node:http';
import type { AddressInfo } from 'node:net';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { chromium } from '@playwright/test';
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
});
