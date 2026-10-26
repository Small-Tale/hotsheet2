import { mkdir, mkdtemp, readdir, readFile, rm } from 'node:fs/promises';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { getRequestListener } from '@hono/node-server';
import { type BrowserContext, expect, type Page } from '@playwright/test';
import { Hono } from 'hono';

import type { FullTicket } from '../src/api';
import { DESKTOP_DRAFT_FILES_DIRECTORY, desktopStorageBridge } from '../src/desktop-storage-bridge';
import { realTicketServer } from './real-ticket-server';

// HS2-791W19: the packaged desktop app serves each launch from a new ephemeral loopback origin.
// Staged composer attachments must come back from the desktop bridge's native app-data records,
// not from origin-scoped IndexedDB, survive a failed save or read without being dropped, and leave
// nothing behind once removed or submitted. Synthetic file contents only; no customer content.

type Fault = (request: { method: string; key: string }) => boolean | 404;

export async function findFile(directory: string, name: string): Promise<string | undefined> {
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) {
      const found = await findFile(path, name);
      if (found) return found;
    } else if (entry.name === name) return path;
  }
  return undefined;
}

/** A real ticket server, real bridge handlers per launch, and one shared native app-data directory. */
export async function desktopApp(context: BrowserContext, baseURL: string) {
  const server = await realTicketServer(),
    appData = await mkdtemp(join(tmpdir(), 'hs2-desktop-drafts-e2e-')),
    preferences = join(appData, 'browser-preferences-v1'),
    drafts = join(preferences, DESKTOP_DRAFT_FILES_DIRECTORY),
    apiPath = `/__hotsheet/project-api/${server.checkoutId}`;
  const openedProjects = [
    { id: server.checkoutId, root: server.root, name: 'drafts', stores: [server.store], apiPath },
  ];
  let bridge = desktopStorageBridge(preferences),
    fault: Fault = () => false,
    bridgeGate: (method: string, key: string) => Promise<void> = () => Promise.resolve(),
    apiGate: (method: string, path: string) => Promise<void> = () => Promise.resolve(),
    apiFault: (method: string, path: string) => boolean = () => false;
  function pause(
    set: (gate: (method: string, key: string) => Promise<void>) => void,
    matches: (method: string, key: string) => boolean,
  ) {
    let start!: () => void, release!: () => void;
    const started = new Promise<void>((resolve) => {
        start = resolve;
      }),
      resumed = new Promise<void>((resolve) => {
        release = resolve;
      });
    set(async (method, key) => {
      if (matches(method, key)) {
        start();
        await resumed;
      }
    });
    return {
      started,
      release: () => {
        set(() => Promise.resolve());
        release();
      },
    };
  }
  await context.addInitScript(() => {
    (window as unknown as Record<string, unknown>).__HOTSHEET_DESKTOP_STORAGE__ = true;
  });
  await context.routeWebSocket(/.*/, (route) => {
    void route.close();
  });
  // A real HTTP bridge is essential for pagehide: Playwright route callbacks can be torn
  // down before an unloading document's keepalive is delivered (HS2-791W19).
  const host = new Hono();
  host.all('*', async (context) => {
    const request = context.req.raw,
      url = new URL(request.url);
    if (/^\/__hotsheet\/desktop-(storage|draft-files)$/u.test(url.pathname)) {
      const key = url.searchParams.get('key') ?? '',
        failure = key ? fault({ method: request.method, key }) : false;
      if (failure) {
        await bridgeGate(request.method, key);
        return new Response(JSON.stringify({ error: 'Could not access desktop preferences.' }), {
          status: failure === 404 ? 404 : 500,
          headers: { 'Content-Type': 'application/json' },
        });
      }
      const response = await bridge.fetch(request);
      await bridgeGate(request.method, key);
      return response;
    }
    const upstream = await fetch(new URL(url.pathname + url.search, baseURL), {
      method: request.method,
      ...(request.method === 'GET' || request.method === 'HEAD' ? {} : { body: await request.arrayBuffer() }),
    });
    const headers = new Headers(upstream.headers);
    // fetch decoded the upstream response; the proxy returns those decoded bytes.
    headers.delete('content-encoding');
    headers.delete('content-length');
    return new Response(await upstream.arrayBuffer(), { status: upstream.status, headers });
  });
  const handle = getRequestListener(host.fetch),
    listener = createServer((request, response) => {
      void handle(request, response);
    });
  await new Promise<void>((resolve) => {
    listener.listen(0, '127.0.0.1', resolve);
  });
  const address = listener.address();
  if (!address || typeof address === 'string') throw new Error('Synthetic bridge did not bind a TCP port.');
  const appOrigin = `http://127.0.0.1:${String(address.port)}`;
  await context.route('**/__hotsheet/projects/open', (route) => {
    const root = route.request().postDataJSON().root as string;
    return route.fulfill({
      status: 201,
      json: openedProjects.find((project) => project.root === root) ?? openedProjects[0],
    });
  });
  await context.route('**/__hotsheet/project-api/**', async (route) => {
    const incoming = new URL(route.request().url()),
      path = incoming.pathname.replace(/^\/__hotsheet\/project-api\/[^/]+/u, '');
    try {
      if (apiFault(route.request().method(), path)) {
        await apiGate(route.request().method(), path);
        await route.fulfill({ status: 500, json: { error: 'Synthetic operation rejected.' } });
        return;
      }
      const response = await route.fetch({
        url: `${server.url}${path}${incoming.search}`,
        headers: { ...route.request().headers(), 'X-Hotsheet-Secret': server.secret },
      });
      await apiGate(route.request().method(), path);
      await route.fulfill({ response });
    } catch {
      await route.abort().catch(() => undefined);
    }
  });
  let launches = 0;
  return {
    server,
    failApiWhen(matches: (method: string, path: string) => boolean) {
      apiFault = matches;
    },
    async completedTicket(title = 'Synthetic completed ticket') {
      const ticket = await server.request<FullTicket>(`/checkouts/${server.checkoutId}/tickets`, 'POST', {
        title,
        category: 'task',
      });
      await server.request(
        `/checkouts/${server.checkoutId}/tickets/${encodeURIComponent(ticket.connection_id + ':' + ticket.id)}`,
        'PATCH',
        { status: 'completed', expected_token: ticket.concurrency_token },
      );
      return ticket;
    },
    pauseBridge: (matches: (method: string, key: string) => boolean) =>
      pause((gate) => {
        bridgeGate = gate;
      }, matches),
    pauseApi: (matches: (method: string, path: string) => boolean) =>
      pause((gate) => {
        apiGate = gate;
      }, matches),
    async addProject(page: Page) {
      const root = join(appData, 'other-project');
      await mkdir(root, { recursive: true });
      const checkout = await server.request<{ id: string }>('/checkouts', 'POST', { root, stores: [server.store] });
      openedProjects.push({
        id: checkout.id,
        root,
        name: 'other-project',
        stores: [server.store],
        apiPath: `/__hotsheet/project-api/${checkout.id}`,
      });
      await page.route('**/__hotsheet/folders/choose', (route) => route.fulfill({ json: { path: root } }));
      await page.getByRole('button', { name: 'Add Project', exact: true }).dispatchEvent('click');
      await expect(page.getByRole('tab', { name: 'other-project', exact: true })).toHaveAttribute(
        'aria-selected',
        'true',
      );
      return checkout.id;
    },
    records: async () => (await readdir(drafts).catch(() => [])).filter((name) => name.endsWith('.draft')),
    failWhen(next: Fault) {
      fault = next;
    },
    /** Quit and relaunch: a new loopback origin (fresh localStorage and IndexedDB) and a new bridge. */
    async launch(page?: Page) {
      await page?.close();
      bridge = desktopStorageBridge(preferences);
      const origin = new URL(appOrigin);
      // Alternate loopback hosts so every launch is a different browser origin.
      origin.hostname = launches++ % 2 ? 'localhost' : '127.0.0.1';
      const next = await context.newPage();
      await next.setViewportSize({ width: 1280, height: 900 });
      await next.goto(origin.href);
      return next;
    },
    async storedSession(projectId = server.checkoutId) {
      const entries = (await (await bridge.request(new URL('/__hotsheet/desktop-storage', baseURL).href)).json()) as {
        key: string;
        value: string | null;
      }[];
      return (
        entries.find(
          (entry) => entry.key.startsWith('hotsheet.workspace.project-session.v1.') && entry.key.includes(projectId),
        )?.value ?? ''
      );
    },
    async stop() {
      listener.closeAllConnections();
      await new Promise<void>((resolve, reject) => {
        listener.close((reason) => {
          if (reason) reject(reason);
          else resolve();
        });
      });
      await server.stop();
      await rm(appData, { recursive: true, force: true });
    },
  };
}

export const dialog = (page: Page) => page.getByRole('dialog', { name: 'Create ticket' });
export const responded = (page: Page, method: string, count = 1) => {
  let seen = 0;
  return page.waitForResponse(
    (response) =>
      response.url().includes('/__hotsheet/desktop-draft-files') &&
      response.request().method() === method &&
      ++seen === count,
  );
};
export const sessionSaved = (page: Page, text: string) =>
  page.waitForResponse(
    (response) =>
      response.url().endsWith('/__hotsheet/desktop-storage') &&
      response.request().method() === 'PUT' &&
      response.status() === 204 &&
      (response.request().postData() ?? '').includes(text),
  );
export async function openNewTicket(page: Page, first = false) {
  if (first) {
    await page.getByRole('button', { name: 'Open Project' }).click();
    await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();
  }
  await page.getByRole('button', { name: 'New ticket…' }).click();
  await expect(dialog(page)).toBeVisible();
}
export const stage = (page: Page, files: { name: string; bytes: number[] }[]) =>
  dialog(page)
    .getByLabel('Browse attachments for new ticket', { exact: true })
    .setInputFiles(
      files.map((file) => ({ name: file.name, mimeType: 'application/octet-stream', buffer: Buffer.from(file.bytes) })),
    );
export async function createAndExpectStored(
  page: Page,
  app: Awaited<ReturnType<typeof desktopApp>>,
  files: Record<string, number[]>,
) {
  await dialog(page).getByRole('button', { name: 'Create ticket', exact: true }).click();
  await expect(dialog(page)).toBeHidden();
  for (const [name, bytes] of Object.entries(files)) {
    await expect.poll(() => findFile(app.server.store, name), { timeout: 20_000 }).toBeDefined();
    expect([...(await readFile((await findFile(app.server.store, name))!))]).toEqual(bytes);
  }
  await expect.poll(() => app.records()).toEqual([]);
}
/** Real-app review of a restored draft at wide, medium, and narrow widths, then back to wide. */
export async function captureWidths(page: Page, state: string, surface = dialog(page)) {
  for (const [label, width] of [
    ['wide', 1280],
    ['medium', 900],
    ['narrow', 390],
    ['wide-again', 1280],
  ] as const) {
    await page.setViewportSize({ width, height: 900 });
    // Review the settled layout, not a rail still sliding after the resize.
    await page.waitForFunction(() =>
      document
        .getAnimations()
        .every(
          (animation) =>
            animation.playState !== 'running' || animation.effect?.getComputedTiming().endTime === Infinity,
        ),
    );
    const box = await surface.boundingBox();
    expect(box?.width).toBeGreaterThan(0);
    expect((box?.x ?? -1) + (box?.width ?? 0)).toBeLessThanOrEqual(width + 1);
    await page.screenshot({ path: `target/visual-captures/hs2-791w19-${state}-${label}-${String(width)}.png` });
    const native = surface.locator('dialog');
    // A native dialog supplied directly (Not working) is already the component bounds.
    await ((await native.count()) ? native : surface).screenshot({
      path: `target/visual-captures/hs2-791w19-${state}-${label}-${String(width)}-component.png`,
    });
  }
}

export async function openNotWorking(page: Page, ticket: FullTicket) {
  await page.locator(`[data-ticket-slug="${ticket.slug}"]`).first().click({ button: 'right' });
  await page.getByRole('menuitem', { name: 'Not Working…', exact: true }).click();
  const surface = page.getByRole('dialog', { name: `Not Working — ${ticket.slug}` });
  await expect(surface.getByRole('textbox')).toBeVisible();
  return surface;
}
