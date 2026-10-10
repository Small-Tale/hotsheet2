import { mkdirSync, writeFileSync } from 'node:fs';

import { expect, test } from '@playwright/test';

import { realTicketServer } from './real-ticket-server';

test.use({ browserName: 'firefox' });

const PYTHON_PROFILE = [
  'import sys,time',
  'while True:',
  '    line = sys.stdin.readline()',
  '    if not line:',
  '        break',
  "    if line.strip() != 'GO':",
  '        continue',
  '    for index in range(600):',
  "        marker = f'FRAME:{index:04d}:{time.time_ns() // 1000000:013d} '",
  "        sys.stdout.write(marker + 'x' * 90 + chr(13) + chr(10) + ('x' * 110 + chr(13) + chr(10)) * 15)",
  '        sys.stdout.flush()',
  '        time.sleep(0.003)',
].join('\n');

type Marker = { index: number; processEpochMs: number; browserEpochMs: number };
type ProfileState = {
  socket?: WebSocket;
  markers: Marker[];
  frameGapsMs: number[];
  active: boolean;
  tail: string;
};

declare global {
  interface Window {
    __firefoxPtyProfile?: ProfileState;
  }
}

function percentile(values: number[], portion: number): number {
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.ceil(sorted.length * portion) - 1];
}

for (const viewport of [
  { name: 'desktop', width: 1440, height: 900, forceDom: false },
  { name: 'narrow', width: 390, height: 844, forceDom: false },
  { name: 'desktop-dom', width: 1440, height: 900, forceDom: true },
]) {
  test('profiles Firefox real PTY output at ' + viewport.name + ' (HS2-527G7P)', async ({ page }, testInfo) => {
    test.skip(process.env.HOTSHEET_FIREFOX_PTY_PROFILE !== '1', 'Run npm run profile:pty:firefox.');
    test.setTimeout(120_000);
    const server = await realTicketServer();
    try {
      const terminal = await server.request<{ id: string }>('/terminals', 'POST', {
        command: 'python3',
        args: ['-u', '-c', PYTHON_PROFILE],
        cwd: server.root,
      });
      await page.setViewportSize({ width: viewport.width, height: viewport.height });
      await page.addInitScript((forceDom) => {
        if (forceDom) {
          Object.defineProperty(navigator, 'userAgent', {
            configurable: true,
            get: () => 'Mozilla/5.0 AppleWebKit/605.1.15 Version/18.0 Safari/605.1.15',
          });
        }
        const state: ProfileState = { markers: [], frameGapsMs: [], active: false, tail: '' };
        window.__firefoxPtyProfile = state;
        const NativeWebSocket = window.WebSocket;
        window.WebSocket = class extends NativeWebSocket {
          constructor(url: string | URL, protocols?: string | string[]) {
            super(url, protocols ?? []);
            if (!String(url).includes('/terminals/')) return;
            state.socket = this;
            const seen = new Set<number>();
            this.addEventListener('message', (event) => {
              if (!state.active || !(event.data instanceof ArrayBuffer)) return;
              const joined = state.tail + new TextDecoder().decode(event.data);
              for (const match of joined.matchAll(/FRAME:(\d{4}):(\d{13})/g)) {
                const index = Number(match[1]);
                if (seen.has(index)) continue;
                seen.add(index);
                state.markers.push({
                  index,
                  processEpochMs: Number(match[2]),
                  browserEpochMs: Date.now(),
                });
              }
              state.tail = joined.slice(-48);
            });
          }
        };
        let previous = 0;
        const frame = (now: number) => {
          if (state.active && previous) state.frameGapsMs.push(now - previous);
          previous = now;
          requestAnimationFrame(frame);
        };
        requestAnimationFrame(frame);
      }, viewport.forceDom);
      await page.goto('/?dev-review=false');
      const socketUrl =
        server.url.replace(/^http/, 'ws') +
        '/terminals/' +
        encodeURIComponent(terminal.id) +
        '/attach?secret=' +
        encodeURIComponent(server.secret);
      await page.evaluate(async (url) => {
        const overlay = document.createElement('section');
        overlay.style.cssText = 'position:fixed;inset:0;z-index:2147483647;background:#111;';
        const viewportElement = document.createElement('div');
        viewportElement.className = 'terminal-viewport terminal-viewport--dedicated';
        viewportElement.dataset.component = 'terminal-viewport';
        viewportElement.dataset.displayMode = 'interactive';
        viewportElement.style.cssText = 'width:100%;height:100%;box-sizing:border-box;';
        overlay.append(viewportElement);
        document.body.append(overlay);
        const modulePath = '/src/terminal-viewport.ts';
        const { mountTerminalViewport } = await import(/* @vite-ignore */ modulePath);
        mountTerminalViewport(viewportElement, { url });
      }, socketUrl);
      const surface = page.locator('.terminal-viewport--dedicated').last();
      await expect(surface).toHaveAttribute('data-connection', 'connected');
      const renderer = await surface.getAttribute('data-renderer');
      const started = await page.evaluate(() => {
        const state = window.__firefoxPtyProfile!;
        state.active = true;
        state.socket!.send('GO\n');
        return performance.now();
      });
      await expect
        .poll(() => page.evaluate(() => window.__firefoxPtyProfile!.markers.length), { timeout: 90_000 })
        .toBe(600);
      const measurement = await page.evaluate((start) => {
        const state = window.__firefoxPtyProfile!;
        state.active = false;
        return {
          elapsedMs: performance.now() - start,
          markers: state.markers,
          frameGapsMs: state.frameGapsMs,
        };
      }, started);
      const lag = measurement.markers.map((marker) => marker.browserEpochMs - marker.processEpochMs);
      const summary = {
        viewport,
        renderer,
        markerCount: measurement.markers.length,
        processToBrowserMs: { p50: percentile(lag, 0.5), p95: percentile(lag, 0.95) },
        frameGapsOver50Ms: measurement.frameGapsMs.filter((gap) => gap > 50).length,
        elapsedMs: measurement.elapsedMs,
        markers: measurement.markers,
        frameGapsMs: measurement.frameGapsMs,
      };
      mkdirSync('target/performance-traces', { recursive: true });
      const prefix = 'target/performance-traces/hs2-527g7p-' + viewport.name;
      writeFileSync(prefix + '-metrics.json', JSON.stringify(summary, null, 2));
      writeFileSync(prefix + '-after.png', await surface.screenshot({ animations: 'disabled' }));
      await testInfo.attach(viewport.name + '-firefox-metrics.json', {
        path: prefix + '-metrics.json',
        contentType: 'application/json',
      });
      await testInfo.attach(viewport.name + '-firefox-after.png', {
        path: prefix + '-after.png',
        contentType: 'image/png',
      });
      await page.evaluate(async () => {
        const modulePath = '/src/terminal-viewport.ts';
        const { TERMINAL_VIEWPORT_PARK_EVENT } = await import(/* @vite-ignore */ modulePath);
        document.querySelector('.terminal-viewport--dedicated')!.dispatchEvent(new Event(TERMINAL_VIEWPORT_PARK_EVENT));
      });
      await expect(surface).toHaveAttribute('data-renderer', 'dom');
      await page.evaluate(async () => {
        const modulePath = '/src/terminal-viewport.ts';
        const { TERMINAL_VIEWPORT_RESUME_EVENT } = await import(/* @vite-ignore */ modulePath);
        document
          .querySelector('.terminal-viewport--dedicated')!
          .dispatchEvent(new CustomEvent(TERMINAL_VIEWPORT_RESUME_EVENT, { detail: { focus: false } }));
      });
      await expect(surface).toHaveAttribute('data-renderer', renderer!);
      await expect(surface).toHaveAttribute('data-connection', 'connected');
      writeFileSync(prefix + '-resumed.png', await surface.screenshot({ animations: 'disabled' }));
      await testInfo.attach(viewport.name + '-firefox-resumed.png', {
        path: prefix + '-resumed.png',
        contentType: 'image/png',
      });
      expect(summary.markerCount).toBe(600);
    } finally {
      await server.stop();
    }
  });
}
