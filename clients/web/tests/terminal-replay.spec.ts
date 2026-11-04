import { mkdirSync, writeFileSync } from 'node:fs';

import { expect, type Page, test, webkit } from '@playwright/test';

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
    /** While set, socket messages from this index on are held until released. */
    __holdFromMessage?: number;
    __heldMessages?: (() => void)[];
  }
}

async function recordSockets(page: Page) {
  await page.addInitScript(() => {
    window.__replayFrames = [];
    window.__replayUrls = [];
    window.__replaySockets = [];
    const NativeWebSocket = window.WebSocket;
    window.WebSocket = class extends NativeWebSocket {
      private readonly indexes = new WeakMap<Event, number>();
      private received = 0;
      override addEventListener(
        type: string,
        listener: EventListenerOrEventListenerObject | null,
        options?: boolean | AddEventListenerOptions,
      ): void {
        const add = (handler: EventListenerOrEventListenerObject | null) => {
          EventTarget.prototype.addEventListener.call(this, type, handler, options);
        };
        if (type !== 'message' || listener === null) {
          add(listener);
          return;
        }
        const deliver = (event: Event) => {
          if (typeof listener === 'function') listener.call(this, event);
          else listener.handleEvent(event);
        };
        add((event) => {
          let index = this.indexes.get(event);
          if (index === undefined) {
            index = this.received++;
            this.indexes.set(event, index);
          }
          const hold = window.__holdFromMessage;
          if (hold !== undefined && index >= hold) {
            (window.__heldMessages ??= []).push(() => {
              deliver(event);
            });
            return;
          }
          deliver(event);
        });
      }
      constructor(url: string | URL, protocols?: string | string[]) {
        const terminal = String(url).includes('/terminals/');
        super(terminal && window.__blockReplaySockets === true ? 'ws://127.0.0.1:9/blocked' : url, protocols ?? []);
        if (!terminal || window.__blockReplaySockets === true) return;
        window.__replaySockets?.push(this);
        window.__replayUrls?.push(String(url));
        this.addEventListener('message', (event: Event) => {
          const { data } = event as MessageEvent<unknown>;
          window.__replayFrames?.push(
            typeof data === 'string'
              ? { at: performance.now(), text: data }
              : { at: performance.now(), bytes: (data as ArrayBuffer).byteLength },
          );
        });
      }
    };
  });
}

async function mountViewport(page: Page, url: string, progressiveReplay?: boolean) {
  await page.evaluate(
    async ({ socketUrl, progressive }) => {
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
      mountTerminalViewport(element, {
        url: socketUrl,
        autoFocus: true,
        ...(progressive === undefined ? {} : { progressiveReplay: progressive }),
      });
    },
    { socketUrl: url, progressive: progressiveReplay },
  );
  return page.locator('.terminal-viewport--dedicated').last();
}

for (const viewport of [
  { name: 'wide', width: 1440, height: 900, browserName: 'chromium' },
  { name: 'phone', width: 390, height: 844, browserName: 'chromium' },
  // iOS Safari decodes deflate-raw since 16.4; WebKit is the phone's real engine.
  { name: 'phone-webkit', width: 390, height: 844, browserName: 'webkit' },
] as const) {
  test(`paints a capped compressed replay, then live output, at ${viewport.name} (HS2-K93QHA)`, async ({
    page: chromiumPage,
    baseURL,
  }, testInfo) => {
    test.setTimeout(90_000);
    const server = await realTicketServer();
    const webkitBrowser = viewport.browserName === 'webkit' ? await webkit.launch() : undefined;
    const page = webkitBrowser ? await webkitBrowser.newPage({ baseURL }) : chromiumPage;
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
      await webkitBrowser?.close();
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

/**
 * Paint the tail, read back into it, release the whole replay, and reach far older history.
 * `cpuThrottle` slows the page's CPU the way a slow CI host does (HS2-J6B4MT).
 */
async function paintTailThenWholeReplay(page: Page, cpuThrottle?: number) {
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
    await page.addInitScript(() => {
      // Hold everything after the tail (header + payload) until the test releases it.
      window.__holdFromMessage = 2;
    });
    await page.goto('/');
    if (cpuThrottle !== undefined) {
      const cdp = await page.context().newCDPSession(page);
      await cdp.send('Emulation.setCPUThrottlingRate', { rate: cpuThrottle });
    }
    const surface = await mountViewport(
      page,
      `${server.url.replace(/^http/, 'ws')}/terminals/${encodeURIComponent(terminal.id)}/attach?secret=${encodeURIComponent(server.secret)}`,
      true,
    );
    const rows = surface.locator('.xterm-rows');
    await expect(rows).toContainText('REPLAY-READY');
    const url = new URL((await page.evaluate(() => window.__replayUrls ?? []))[0]);
    expect(url.searchParams.get('replay_tail_bytes')).toBe('32768');
    const tailHeader = JSON.parse((await page.evaluate(() => window.__replayFrames ?? []))[0].text ?? '{}') as {
      terminal_frame?: { kind: string; bytes: number };
    };
    expect(tailHeader.terminal_frame?.kind).toBe('tail');
    expect(tailHeader.terminal_frame?.bytes).toBeLessThanOrEqual(32768);

    // Read back into the tail, then let the whole replay arrive.
    await surface.hover();
    await page.mouse.wheel(0, -600);
    await expect(rows).not.toContainText('REPLAY-READY');
    const before = (await rows.innerText()).split('\n').filter(Boolean);
    mkdirSync('target/visual-captures', { recursive: true });
    writeFileSync(
      'target/visual-captures/hs2-k93qha-tail-phone.png',
      await surface.screenshot({ animations: 'disabled' }),
    );
    // The scrollbar's slider shows how much history the emulator holds: about 1,170 lines for the
    // tail alone (about 43 px here), at least 2,500 once the whole replay is parsed (xterm's
    // 20 px minimum here). The tail's slider sits well above that minimum so it can shrink.
    const slider = surface.locator('.xterm-scrollable-element > .scrollbar.vertical > .slider');
    const sliderHeight = async () => (await slider.boundingBox())?.height ?? Number.NaN;
    const tailSliderHeight = await sliderHeight();
    expect(tailSliderHeight).toBeGreaterThan(30);
    await page.evaluate(() => {
      window.__holdFromMessage = undefined;
      for (const release of (window.__heldMessages ?? []).splice(0)) release();
    });
    await expect
      .poll(async () =>
        (await page.evaluate(() => window.__replayFrames ?? [])).some((frame) => frame.text?.includes('"replace"')),
      )
      .toBe(true);
    // Wait until the whole replay is parsed, not only received. Its payload decodes and parses
    // asynchronously, so on a slow host the tail stays on screen well after the frame arrives,
    // and the check below would pass before the replacement ran (HS2-J6B4MT).
    await expect.poll(sliderHeight, { timeout: 60_000 }).toBeLessThan(tailSliderHeight * 0.75);
    // The same lines stay on screen: the reader's distance from the bottom is kept.
    await expect.poll(async () => (await rows.innerText()).split('\n').filter(Boolean)).toEqual(before);
    writeFileSync(
      `target/visual-captures/hs2-k93qha-whole-phone${cpuThrottle === undefined ? '' : '-throttled'}.png`,
      await surface.screenshot({ animations: 'disabled' }),
    );

    // The whole history is there now: paging up reaches lines far older than the 32 KiB tail held
    // (about the last 1,170 lines), down to the phone's scrollback limit. Each Shift+PageUp moves
    // a whole screen, so the climb takes a few dozen keys at any host speed. Bursts of wheel
    // events each moved only about four rows, and a slow macOS host ran out of poll time two or
    // three bursts in, short of the start (HS2-J6B4MT).
    const tailFirstLine = 4_000 - Math.ceil(32_768 / '\u001b[32mreplay line 00000\u001b[0m\r\n'.length);
    const topLine = async () => Number(/replay line (\d{5})/.exec(await rows.innerText())?.[1] ?? Infinity);
    await surface.locator('.xterm-helper-textarea').focus();
    for (let current = await topLine(); current >= tailFirstLine - 500; current = await topLine()) {
      await page.keyboard.press('Shift+PageUp');
      // Each page lands before the next key: wait on the screen, not on a time budget. At the top
      // of a capped history the screen stops moving and this fails.
      await expect.poll(topLine).toBeLessThan(current);
    }
  } finally {
    await server.stop();
  }
}

test('paints the replay tail first, then the whole replay keeps the reader in place (HS2-K93QHA)', async ({ page }) => {
  test.setTimeout(90_000);
  await paintTailThenWholeReplay(page);
});

test('on a slow host the whole replay still reaches far older history (HS2-J6B4MT)', async ({ page, browserName }) => {
  test.skip(browserName !== 'chromium', 'CPU throttling is a Chromium DevTools capability.');
  test.setTimeout(180_000);
  // Twenty times slower than this host reproduces the macOS runner's failure under the old wheel
  // bursts: two bursts, then a timeout at line 3225.
  await paintTailThenWholeReplay(page, 20);
});
