import { mkdirSync, writeFileSync } from 'node:fs';

import { expect, type Page, test } from '@playwright/test';

import { realTicketServer } from './real-ticket-server';

// HS2-K93QHA: a mounted terminal viewport negotiates a capped, compressed attach replay with a
// real server and paints it, then keeps streaming live output in order.

const LINES = 4_000;
const PRINTER = [
  'import sys',
  `for index in range(${LINES}):`,
  "    sys.stdout.write('\\x1b[32mreplay line %05d\\x1b[0m\\r\\n' % index)",
  "sys.stdout.write('REPLAY-READY\\r\\n')",
  'sys.stdout.flush()',
  'for line in sys.stdin:',
  "    sys.stdout.write('echo:' + line.strip() + '\\r\\n')",
  '    sys.stdout.flush()',
].join('\n');

interface Frame {
  at: number;
  text?: string;
  bytes?: number;
}
declare global {
  interface Window {
    __replayFrames?: Frame[];
    __replayUrls?: string[];
    __replaySockets?: WebSocket[];
    /** While set, new terminal sockets fail to connect, holding the viewer disconnected. */
    __blockReplaySockets?: boolean;
  }
}

async function recordSockets(page: Page) {
  await page.addInitScript(() => {
    window.__replayFrames = [];
    window.__replayUrls = [];
    window.__replaySockets = [];
    const NativeWebSocket = window.WebSocket;
    window.WebSocket = class extends NativeWebSocket {
      constructor(url: string | URL, protocols?: string | string[]) {
        const terminal = String(url).includes('/terminals/');
        super(terminal && window.__blockReplaySockets === true ? 'ws://127.0.0.1:9/blocked' : url, protocols ?? []);
        if (!terminal || window.__blockReplaySockets === true) return;
        window.__replaySockets?.push(this);
        window.__replayUrls?.push(String(url));
        this.addEventListener('message', (event: MessageEvent) => {
          window.__replayFrames?.push(
            typeof event.data === 'string'
              ? { at: performance.now(), text: event.data }
              : { at: performance.now(), bytes: (event.data as ArrayBuffer).byteLength },
          );
        });
      }
    };
  });
}

async function mountViewport(page: Page, url: string) {
  await page.evaluate(async (socketUrl) => {
    const overlay = document.createElement('section');
    overlay.style.cssText = 'position:fixed;inset:0;z-index:2147483647;background:#111;';
    const element = document.createElement('div');
    element.className = 'terminal-viewport terminal-viewport--dedicated';
    element.dataset.component = 'terminal-viewport';
    element.dataset.displayMode = 'interactive';
    element.style.cssText = 'width:100%;height:100%;box-sizing:border-box;';
    overlay.append(element);
    document.body.append(overlay);
    const modulePath = '/src/terminal-viewport.ts';
    const { mountTerminalViewport } = await import(/* @vite-ignore */ modulePath);
    mountTerminalViewport(element, { url: socketUrl, autoFocus: true });
  }, url);
  return page.locator('.terminal-viewport--dedicated').last();
}

for (const viewport of [
  { name: 'wide', width: 1440, height: 900 },
  { name: 'phone', width: 390, height: 844 },
]) {
  test(`paints a capped compressed replay, then live output, at ${viewport.name} (HS2-K93QHA)`, async ({
    page,
  }, testInfo) => {
    test.setTimeout(90_000);
    const server = await realTicketServer();
    try {
      const terminal = await server.request<{ id: string }>('/terminals', 'POST', {
        command: 'python3',
        args: ['-u', '-c', PRINTER],
        cwd: server.root,
      });
      await expect
        .poll(async () => (await server.request<{ scrollback?: string }>(`/terminals/${terminal.id}`)).scrollback ?? '')
        .toContain('REPLAY-READY');
      await page.setViewportSize({ width: viewport.width, height: viewport.height });
      await recordSockets(page);
      await page.goto('/');
      const socketUrl =
        server.url.replace(/^http/, 'ws') +
        '/terminals/' +
        encodeURIComponent(terminal.id) +
        '/attach?secret=' +
        encodeURIComponent(server.secret);
      const surface = await mountViewport(page, socketUrl);
      await expect(surface).toHaveAttribute('data-connection', 'connected');
      await expect(surface.locator('.xterm-rows')).toContainText('REPLAY-READY');

      const url = new URL((await page.evaluate(() => window.__replayUrls ?? []))[0]);
      const scrollback = Number(await surface.getAttribute('data-scrollback-limit'));
      expect(url.searchParams.get('replay_protocol')).toBe('2');
      expect(url.searchParams.get('replay_encoding')).toBe('deflate-raw');
      const maxBytes = Number(url.searchParams.get('replay_max_bytes'));
      expect(maxBytes).toBeGreaterThanOrEqual(scrollback * 192);

      const frames = await page.evaluate(() => window.__replayFrames ?? []);
      const header = JSON.parse(frames[0].text ?? '{}') as {
        terminal_frame?: { kind: string; encoding: string; bytes: number };
      };
      expect(header.terminal_frame?.kind).toBe('replay');
      expect(header.terminal_frame?.encoding).toBe('deflate-raw');
      expect(header.terminal_frame?.bytes).toBeLessThanOrEqual(maxBytes);
      expect(frames[1].bytes).toBeLessThan((header.terminal_frame?.bytes ?? 0) / 3);

      // Live output after the decoded replay arrives in order, and typing still reaches the PTY.
      await surface.locator('.xterm-helper-textarea').focus();
      await page.keyboard.type('live-after-replay');
      await page.keyboard.press('Enter');
      await expect(surface.locator('.xterm-rows')).toContainText('echo:live-after-replay');
      const rows = await surface.locator('.xterm-rows').innerText();
      expect(rows.indexOf('REPLAY-READY')).toBeLessThan(rows.indexOf('echo:live-after-replay'));
      mkdirSync('target/visual-captures', { recursive: true });
      const capture = `target/visual-captures/hs2-k93qha-replay-${viewport.name}.png`;
      writeFileSync(capture, await surface.screenshot({ animations: 'disabled' }));
      await testInfo.attach(`${viewport.name}-replay.png`, { path: capture, contentType: 'image/png' });
    } finally {
      await server.stop();
    }
  });
}

test('a reconnect resumes with only the output it missed (HS2-K93QHA)', async ({ page }) => {
  test.setTimeout(90_000);
  const server = await realTicketServer();
  try {
    const terminal = await server.request<{ id: string }>('/terminals', 'POST', {
      command: 'python3',
      args: ['-u', '-c', PRINTER],
      cwd: server.root,
    });
    await expect
      .poll(async () => (await server.request<{ scrollback?: string }>(`/terminals/${terminal.id}`)).scrollback ?? '')
      .toContain('REPLAY-READY');
    await page.setViewportSize({ width: 390, height: 844 });
    await recordSockets(page);
    await page.goto('/');
    const surface = await mountViewport(
      page,
      `${server.url.replace(/^http/, 'ws')}/terminals/${encodeURIComponent(terminal.id)}/attach?secret=${encodeURIComponent(server.secret)}`,
    );
    const rows = surface.locator('.xterm-rows');
    await expect(rows).toContainText('REPLAY-READY');

    // Drop the socket and hold the viewer offline while the terminal produces output.
    await page.evaluate(() => {
      window.__blockReplaySockets = true;
      window.__replayFrames = [];
      window.__replaySockets?.[0]?.close();
    });
    await expect(surface).toHaveAttribute('data-connection', 'reconnecting');
    // The input route answers with an empty body, so post it directly.
    const input = await fetch(`${server.url}/terminals/${terminal.id}/input`, {
      method: 'POST',
      headers: { 'content-type': 'application/json', 'X-Hotsheet-Secret': server.secret },
      body: JSON.stringify({ data: 'while-away\n' }),
    });
    expect(input.ok).toBe(true);
    await expect
      .poll(async () => (await server.request<{ scrollback?: string }>(`/terminals/${terminal.id}`)).scrollback ?? '')
      .toContain('echo:while-away');
    await page.evaluate(() => {
      window.__blockReplaySockets = false;
    });
    await expect(rows).toContainText('echo:while-away');
    await expect(surface).toHaveAttribute('data-connection', 'connected');

    const urls = await page.evaluate(() => window.__replayUrls ?? []);
    expect(urls).toHaveLength(2);
    expect(new URL(urls[1]).searchParams.get('resume_from')).toMatch(/^[0-9a-f]{16}:[1-9]\d*$/);
    const frames = await page.evaluate(() => window.__replayFrames ?? []);
    const header = JSON.parse(frames[0].text ?? '{}') as { terminal_frame?: { kind: string; bytes: number } };
    expect(header.terminal_frame?.kind).toBe('resume');
    expect(header.terminal_frame?.bytes).toBeLessThan(200);
    // Appended, not replaced: the earlier history is still there and nothing repeats.
    const text = await rows.innerText();
    expect(text.split('echo:while-away')).toHaveLength(2);
    expect(text.split('REPLAY-READY')).toHaveLength(2);
    expect(text).toContain('replay line 03999');

    // Live output keeps flowing after the resume.
    await surface.locator('.xterm-helper-textarea').focus();
    await page.keyboard.type('after-resume');
    await page.keyboard.press('Enter');
    await expect(rows).toContainText('echo:after-resume');
  } finally {
    await server.stop();
  }
});
