import { mkdirSync, writeFileSync } from 'node:fs';

import { type BrowserContext, expect, type Locator, type Page, test } from '@playwright/test';

import { realTicketServer } from './real-ticket-server';

// HS2-HA0Z3A: Clear Terminal is shared through the server. Production terminal viewports attached to
// a real server's PTY: the clearing viewport, a second client attached during the clear, a reload
// after it, and a warm viewport that was offline during the clear and reconnects with its old
// resume position. None of them shows the cleared output again, and new output still streams.

const PROGRAM = [
  'import sys',
  'for index in range(200):',
  "    sys.stdout.write('OLD-OUTPUT %03d\\r\\n' % index)",
  "sys.stdout.write('PROMPT$ ')",
  'sys.stdout.flush()',
  'for line in sys.stdin:',
  "    sys.stdout.write('echo:' + line.strip() + '\\r\\nPROMPT$ ')",
  '    sys.stdout.flush()',
].join('\n');

type Server = Awaited<ReturnType<typeof realTicketServer>>;

declare global {
  interface Window {
    __clearSockets?: WebSocket[];
    __clearSocketUrls?: string[];
    /** While set, new terminal sockets fail to connect, holding the viewer offline. */
    __blockClearSockets?: boolean;
    __shareClear?: (terminalId: string) => Promise<number>;
  }
}

async function recordSockets(page: Page) {
  await page.addInitScript(() => {
    window.__clearSockets = [];
    window.__clearSocketUrls = [];
    const NativeWebSocket = window.WebSocket;
    window.WebSocket = class extends NativeWebSocket {
      constructor(url: string | URL, protocols?: string | string[]) {
        const terminal = String(url).includes('/terminals/'),
          blocked = terminal && window.__blockClearSockets === true;
        super(blocked ? 'ws://127.0.0.1:9/blocked' : url, protocols ?? []);
        if (!terminal || blocked) return;
        window.__clearSockets?.push(this);
        window.__clearSocketUrls?.push(String(url));
      }
    };
  });
}

/** A page whose shared clear posts to the real server from the test runner (no browser CORS). */
async function clientPage(context: BrowserContext, server: Server) {
  const page = await context.newPage();
  await recordSockets(page);
  await page.exposeFunction('__shareClear', async (terminalId: string) => {
    const response = await fetch(`${server.url}/terminals/${encodeURIComponent(terminalId)}/clear`, {
      method: 'POST',
      headers: { 'X-Hotsheet-Secret': server.secret },
    });
    if (!response.ok) throw new Error(`clear failed: ${String(response.status)}`);
    return response.status;
  });
  await page.goto('/');
  return page;
}

/** Mount the production terminal viewport with the identity the drawer gives it. */
async function mountViewport(page: Page, server: Server, terminalId: string): Promise<Locator> {
  const url = `${server.url.replace(/^http/, 'ws')}/terminals/${encodeURIComponent(terminalId)}/attach?secret=${encodeURIComponent(server.secret)}`;
  await page.evaluate(
    async ({ socketUrl, id }) => {
      const element = document.createElement('div');
      element.className = 'terminal-viewport terminal-viewport--dedicated';
      element.dataset.component = 'terminal-viewport';
      element.dataset.displayMode = 'interactive';
      element.dataset.projectId = 'project';
      element.dataset.terminalId = id;
      element.style.cssText = 'position:fixed;inset:0;z-index:2147483647;background:#111;box-sizing:border-box;';
      document.body.append(element);
      const modulePath = '/src/terminal-viewport.ts';
      const { mountTerminalViewport } = await import(/* @vite-ignore */ modulePath);
      mountTerminalViewport(element, { url: socketUrl, autoFocus: true });
    },
    { socketUrl: url, id: terminalId },
  );
  const surface = page.locator('[data-component="terminal-viewport"]').last();
  await expect(surface).toHaveAttribute('data-connection', 'connected');
  return surface;
}

/** Clear as the drawer's Clear Terminal does: this page's viewports first, then the server. */
function clearEverywhere(page: Page, terminalId: string) {
  return page.evaluate(async (id) => {
    const modulePath = '/src/terminal-clear.ts';
    const { clearTerminalEverywhere } = await import(/* @vite-ignore */ modulePath);
    return clearTerminalEverywhere(document, { projectId: 'project', terminalId: id }, () => window.__shareClear!(id));
  }, terminalId);
}

const scrollback = async (server: Server, id: string) =>
  (await server.request<{ scrollback?: string }>(`/terminals/${id}`)).scrollback ?? '';

test('a shared clear reaches a second client, a reload, and an offline warm viewport (HS2-HA0Z3A)', async ({
  context,
}, testInfo) => {
  test.setTimeout(120_000);
  const server = await realTicketServer();
  try {
    const terminal = await server.request<{ id: string }>('/terminals', 'POST', {
      command: 'python3',
      args: ['-u', '-c', PROGRAM],
      cwd: server.root,
    });
    await expect.poll(() => scrollback(server, terminal.id)).toContain('PROMPT$');

    const first = await clientPage(context, server);
    await first.setViewportSize({ width: 1280, height: 800 });
    const clearing = await mountViewport(first, server, terminal.id);
    await expect(clearing.locator('.xterm-rows')).toContainText('OLD-OUTPUT 199');

    // A second client stays attached through the clear.
    const second = await clientPage(context, server);
    await second.setViewportSize({ width: 900, height: 700 });
    const watching = await mountViewport(second, server, terminal.id);
    await expect(watching.locator('.xterm-rows')).toContainText('OLD-OUTPUT 199');

    // A warm viewport goes offline before the clear and keeps its old screen and resume position.
    const third = await clientPage(context, server);
    await third.setViewportSize({ width: 390, height: 844 });
    const offline = await mountViewport(third, server, terminal.id);
    await expect(offline.locator('.xterm-rows')).toContainText('OLD-OUTPUT 199');
    await third.evaluate(() => {
      window.__blockClearSockets = true;
      window.__clearSockets?.[0]?.close();
    });
    await expect(offline).toHaveAttribute('data-connection', 'reconnecting');

    // Clear from the first client: it and the second client clear; the prompt line stays.
    expect(await clearEverywhere(first, terminal.id)).toBe('shared');
    for (const surface of [clearing, watching]) {
      await expect(surface.locator('.xterm-rows')).not.toContainText('OLD-OUTPUT');
      await expect(surface.locator('.xterm-rows')).toContainText('PROMPT$');
    }
    // The replay starts at the prompt line and holds nothing before it. A Unix PTY keeps exactly
    // `PROMPT$ `; Windows ConPTY's prompt line is the end of its resize repaint, with erases and a
    // cursor move after it (HS2-4PBGKF).
    const cleared = await scrollback(server, terminal.id);
    expect(cleared).toMatch(process.platform === 'win32' ? /^PROMPT\$/ : /^PROMPT\$ $/);
    expect(cleared).not.toContain('OLD-OUTPUT');

    // Clear then new output: both attached clients receive it.
    await clearing.locator('.xterm-helper-textarea').focus();
    await first.keyboard.type('after-clear');
    await first.keyboard.press('Enter');
    for (const surface of [clearing, watching]) {
      await expect(surface.locator('.xterm-rows')).toContainText('echo:after-clear');
      await expect(surface.locator('.xterm-rows')).not.toContainText('OLD-OUTPUT');
    }

    // The offline warm viewport reconnects with its pre-clear position and gets the cleared replay.
    await third.evaluate(() => {
      window.__blockClearSockets = false;
    });
    await expect(offline).toHaveAttribute('data-connection', 'connected');
    await expect(offline.locator('.xterm-rows')).toContainText('echo:after-clear');
    await expect(offline.locator('.xterm-rows')).not.toContainText('OLD-OUTPUT');
    const urls = await third.evaluate(() => window.__clearSocketUrls ?? []);
    expect(new URL(urls.at(-1) ?? '').searchParams.get('resume_from')).toMatch(/^[0-9a-f]{16}:\d+$/);

    // Reload the second client: its new mount replays only what followed the clear.
    await second.reload();
    const reloaded = await mountViewport(second, server, terminal.id);
    await expect(reloaded.locator('.xterm-rows')).toContainText('echo:after-clear');
    await expect(reloaded.locator('.xterm-rows')).not.toContainText('OLD-OUTPUT');

    // Clear twice: the second clear still reaches the reloaded client and keeps the prompt.
    expect(await clearEverywhere(first, terminal.id)).toBe('shared');
    for (const surface of [clearing, reloaded, offline]) {
      await expect(surface.locator('.xterm-rows')).not.toContainText('echo:after-clear');
      await expect(surface.locator('.xterm-rows')).toContainText('PROMPT$');
    }
    await clearing.locator('.xterm-helper-textarea').focus();
    await first.keyboard.type('after-twice');
    await first.keyboard.press('Enter');
    await expect(reloaded.locator('.xterm-rows')).toContainText('echo:after-twice');

    mkdirSync('target/visual-captures', { recursive: true });
    const capture = 'target/visual-captures/hs2-ha0z3a-second-client-after-clear.png';
    writeFileSync(capture, await reloaded.screenshot({ animations: 'disabled' }));
    await testInfo.attach('second-client-after-clear.png', { path: capture, contentType: 'image/png' });
  } finally {
    await server.stop();
  }
});
