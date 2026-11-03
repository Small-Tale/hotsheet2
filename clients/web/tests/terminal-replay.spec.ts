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
  }
}

async function recordSockets(page: Page) {
  await page.addInitScript(() => {
    window.__replayFrames = [];
    window.__replayUrls = [];
    const NativeWebSocket = window.WebSocket;
    window.WebSocket = class extends NativeWebSocket {
      constructor(url: string | URL, protocols?: string | string[]) {
        super(url, protocols ?? []);
        if (!String(url).includes('/terminals/')) return;
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
