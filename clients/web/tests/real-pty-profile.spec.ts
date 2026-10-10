import { mkdirSync, writeFileSync } from 'node:fs';

import { expect, test } from '@playwright/test';

import { realTicketServer } from './real-ticket-server';

const PYTHON_PROFILE = String.raw`import sys,time
while True:
    line = sys.stdin.readline()
    if not line:
        break
    if line.strip() != 'GO':
        sys.stdout.write('ACK:' + line)
        sys.stdout.flush()
        continue
    for index in range(600):
        marker = f'FRAME:{index:04d}:{time.time_ns() // 1000000:013d} '
        sys.stdout.write(marker + 'x' * 90 + '\r\n' + ('x' * 110 + '\r\n') * 15)
        sys.stdout.flush()
        time.sleep(0.003)
`;

type Marker = { index: number; processEpochMs: number; browserEpochMs: number; browserPerfMs: number };
type BrowserPtyProfile = {
  socket?: WebSocket;
  active: boolean;
  bytes: number;
  messages: number;
  markers: Marker[];
  echoArrivalPerfMs?: number;
  recentOutput: string;
  frameGapsMs: number[];
  longTasksMs: number[];
  messageProcessingMs: number[];
  messageStartPerfMs?: number;
};

declare global {
  interface Window {
    __realPtyProfile?: BrowserPtyProfile;
  }
}

function percentile(values: number[], portion: number): number | null {
  if (!values.length) return null;
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.ceil(sorted.length * portion) - 1];
}

for (const viewport of [
  { name: 'desktop', width: 1440, height: 900 },
  { name: 'narrow', width: 390, height: 844 },
]) {
  test(`profiles real local-server PTY transport and echo at ${viewport.name} (HS2-E035F5)`, async ({
    page,
  }, testInfo) => {
    test.skip(process.env.HOTSHEET_REAL_PTY_PROFILE !== '1', 'Run npm run profile:pty:real.');
    test.setTimeout(180_000);
    const server = await realTicketServer();
    try {
      const terminal = await server.request<{ id: string }>('/terminals', 'POST', {
        command: 'python3',
        args: ['-u', '-c', PYTHON_PROFILE],
        cwd: server.root,
      });
      await page.setViewportSize({ width: viewport.width, height: viewport.height });
      await page.addInitScript(() => {
        const scope = window;
        const state: BrowserPtyProfile = {
          active: false,
          bytes: 0,
          messages: 0,
          markers: [],
          recentOutput: '',
          frameGapsMs: [],
          longTasksMs: [],
          messageProcessingMs: [],
        };
        scope.__realPtyProfile = state;
        const NativeWebSocket = window.WebSocket;
        window.WebSocket = class extends NativeWebSocket {
          constructor(url: string | URL, protocols?: string | string[]) {
            super(url, protocols ?? []);
            if (!String(url).includes('/terminals/')) return;
            state.socket = this;
            const seen = new Set<number>();
            let tail = '';
            this.addEventListener('message', (event) => {
              if (!(event.data instanceof ArrayBuffer)) return;
              const now = performance.now();
              const chunk = new TextDecoder().decode(event.data);
              const joined = tail + chunk;
              state.recentOutput = joined.slice(-500);
              if (state.echoArrivalPerfMs === undefined && joined.includes('ECHO-PROBE')) state.echoArrivalPerfMs = now;
              if (state.active) {
                state.messageStartPerfMs = now;
                state.bytes += event.data.byteLength;
                state.messages += 1;
                for (const match of joined.matchAll(/FRAME:(\d{4}):(\d{13})/g)) {
                  const index = Number(match[1]);
                  if (seen.has(index)) continue;
                  seen.add(index);
                  state.markers.push({
                    index,
                    processEpochMs: Number(match[2]),
                    browserEpochMs: Date.now(),
                    browserPerfMs: now,
                  });
                }
              }
              tail = joined.slice(-48);
            });
          }
        };
        new PerformanceObserver((list) => {
          if (state.active) for (const entry of list.getEntries()) state.longTasksMs.push(entry.duration);
        }).observe({ entryTypes: ['longtask'] });
        let previous = 0;
        const frame = (now: number) => {
          if (state.active && previous) state.frameGapsMs.push(now - previous);
          previous = now;
          requestAnimationFrame(frame);
        };
        requestAnimationFrame(frame);
      });
      await page.goto('/?dev-review=false');
      const socketUrl = `${server.url.replace(/^http/, 'ws')}/terminals/${encodeURIComponent(terminal.id)}/attach?secret=${encodeURIComponent(server.secret)}`;
      await page.evaluate(async (url) => {
        const overlay = document.createElement('section');
        overlay.style.cssText = 'position:fixed;inset:0;z-index:2147483647;background:#111;';
        const viewport = document.createElement('div');
        viewport.className = 'terminal-viewport terminal-viewport--dedicated';
        viewport.dataset.component = 'terminal-viewport';
        viewport.dataset.displayMode = 'interactive';
        viewport.style.cssText = 'width:100%;height:100%;box-sizing:border-box;';
        overlay.append(viewport);
        document.body.append(overlay);
        const modulePath = '/src/terminal-viewport.ts';
        const { mountTerminalViewport } = await import(/* @vite-ignore */ modulePath);
        mountTerminalViewport(viewport, { url, autoFocus: true });
      }, socketUrl);
      const surface = page.locator('.terminal-viewport--dedicated').last();
      await expect(surface).toHaveAttribute('data-connection', 'connected');
      await expect(surface.locator('.xterm-helper-textarea')).toBeAttached();
      await page.evaluate(() => {
        const profile = window.__realPtyProfile!;
        profile.socket!.addEventListener('message', () => {
          if (profile.active && profile.messageStartPerfMs !== undefined) {
            profile.messageProcessingMs.push(performance.now() - profile.messageStartPerfMs);
          }
        });
      });

      const beforeEcho = await surface.screenshot({ animations: 'disabled' });
      await surface.locator('.xterm-helper-textarea').focus();
      const echoStarted = await page.evaluate(() => performance.now());
      await page.keyboard.insertText('ECHO-PROBE');
      await page.keyboard.press('Enter');
      await expect
        .poll(() =>
          page.evaluate(() =>
            JSON.stringify({
              echoArrivalPerfMs: window.__realPtyProfile?.echoArrivalPerfMs,
              socketState: window.__realPtyProfile?.socket?.readyState,
              recentOutput: window.__realPtyProfile?.recentOutput,
            }),
          ),
        )
        .toContain('echoArrivalPerfMs');
      const echoArrival = await page.evaluate(() => window.__realPtyProfile!.echoArrivalPerfMs!);
      await expect
        .poll(async () => !(await surface.screenshot({ animations: 'disabled' })).equals(beforeEcho))
        .toBe(true);
      const echoPaintUpperBoundMs = (await page.evaluate(() => performance.now())) - echoStarted;

      const cdp = await page.context().newCDPSession(page);
      await cdp.send('HeapProfiler.collectGarbage');
      const heapBeforeBytes = (await cdp.send('Runtime.getHeapUsage')).usedSize;
      await cdp.send('Tracing.start', {
        transferMode: 'ReturnAsStream',
        categories: 'devtools.timeline,blink.user_timing,v8,loading,disabled-by-default-devtools.timeline',
        options: 'sampling-frequency=10000',
      });
      const beforeOutput = await surface.screenshot({ animations: 'disabled' });
      const started = await page.evaluate(() => {
        const state = window.__realPtyProfile!;
        state.bytes = 0;
        state.messages = 0;
        state.markers = [];
        state.frameGapsMs = [];
        state.longTasksMs = [];
        state.messageProcessingMs = [];
        state.active = true;
        performance.mark('hs2-profile:real-pty-load:start');
        state.socket!.send('GO\n');
        return performance.now();
      });
      const firstPaint = (async () => {
        await expect
          .poll(async () => !(await surface.screenshot({ animations: 'disabled' })).equals(beforeOutput))
          .toBe(true);
        return (await page.evaluate(() => performance.now())) - started;
      })();
      await expect
        .poll(() => page.evaluate(() => window.__realPtyProfile!.markers.length), { timeout: 90_000 })
        .toBe(600);
      const firstPaintUpperBoundMs = await firstPaint;
      const measurements = await page.evaluate((start) => {
        performance.mark('hs2-profile:real-pty-load:end');
        const state = window.__realPtyProfile!;
        state.active = false;
        return {
          bytes: state.bytes,
          messages: state.messages,
          markers: state.markers,
          frameGapsMs: state.frameGapsMs,
          longTasksMs: state.longTasksMs,
          messageProcessingMs: state.messageProcessingMs,
          elapsedMs: performance.now() - start,
        };
      }, started);
      const complete = new Promise<string>((resolve, reject) => {
        cdp.once('Tracing.tracingComplete', ({ stream }) => {
          if (stream) resolve(stream);
          else reject(new Error('Chromium trace stream is missing'));
        });
      });
      await cdp.send('Tracing.end');
      const stream = await complete;
      const chunks: Buffer[] = [];
      let done = false;
      while (!done) {
        const part = await cdp.send('IO.read', { handle: stream });
        chunks.push(Buffer.from(part.data, part.base64Encoded ? 'base64' : 'utf8'));
        done = part.eof;
      }
      await cdp.send('IO.close', { handle: stream });
      await cdp.send('HeapProfiler.collectGarbage');
      const heapAfterBytes = (await cdp.send('Runtime.getHeapUsage')).usedSize;
      const processToBrowserMs = measurements.markers.map((marker) => marker.browserEpochMs - marker.processEpochMs);
      const summary = {
        viewport,
        renderer: await surface.getAttribute('data-renderer'),
        realServer: true,
        pythonWrites: 600,
        browserWebSocketMessages: measurements.messages,
        browserBytes: measurements.bytes,
        browserBytesPerSecond: (measurements.bytes * 1000) / measurements.elapsedMs,
        burstMs: measurements.elapsedMs,
        processToBrowserMs: { p50: percentile(processToBrowserMs, 0.5), p95: percentile(processToBrowserMs, 0.95) },
        synchronousMessageProcessingMs: {
          p50: percentile(measurements.messageProcessingMs, 0.5),
          p95: percentile(measurements.messageProcessingMs, 0.95),
        },
        echoInputToMessageMs: echoArrival - echoStarted,
        echoPaintUpperBoundMs,
        firstPaintUpperBoundMs,
        frameGapsOver50Ms: measurements.frameGapsMs.filter((gap) => gap > 50).length,
        longestMainThreadTaskMs: Math.max(0, ...measurements.longTasksMs),
        heapBeforeBytes,
        heapAfterBytes,
        heapRetainedDeltaBytes: heapAfterBytes - heapBeforeBytes,
        markers: measurements.markers,
        frameGapsMs: measurements.frameGapsMs,
        longTasksMs: measurements.longTasksMs,
        messageProcessingMs: measurements.messageProcessingMs,
      };
      mkdirSync('target/performance-traces', { recursive: true });
      const prefix = `target/performance-traces/hs2-e035f5-${viewport.name}`;
      writeFileSync(`${prefix}-chromium-trace.json`, Buffer.concat(chunks));
      writeFileSync(`${prefix}-metrics.json`, JSON.stringify(summary, null, 2));
      await testInfo.attach(`${viewport.name}-real-pty-metrics.json`, {
        path: `${prefix}-metrics.json`,
        contentType: 'application/json',
      });
      const screenshotPath = `target/performance-traces/hs2-36p1np-${viewport.name}-terminal-after.png`;
      writeFileSync(screenshotPath, await surface.screenshot({ animations: 'disabled' }));
      await testInfo.attach(`${viewport.name}-terminal-after.png`, {
        path: screenshotPath,
        contentType: 'image/png',
      });
      expect(summary.browserBytes).toBeGreaterThan(1_000_000);
      expect(summary.browserWebSocketMessages).toBeGreaterThan(0);
      expect(summary.processToBrowserMs.p95).not.toBeNull();
      expect(summary.renderer).toBe('dom');
    } finally {
      await server.stop();
    }
  });
}

for (const viewport of [
  { name: 'desktop', width: 1440, height: 900 },
  { name: 'narrow', width: 390, height: 844 },
]) {
  test(`profiles bare real PTY WebSocket transport at ${viewport.name} (HS2-36P1NP)`, async ({ page }, testInfo) => {
    test.skip(process.env.HOTSHEET_REAL_PTY_PROFILE !== '1', 'Run npm run profile:pty:real.');
    test.setTimeout(90_000);
    const server = await realTicketServer();
    try {
      const terminal = await server.request<{ id: string }>('/terminals', 'POST', {
        command: 'python3',
        args: ['-u', '-c', PYTHON_PROFILE],
        cwd: server.root,
      });
      await page.setViewportSize({ width: viewport.width, height: viewport.height });
      await page.goto('/?dev-review=false');
      const socketUrl = `${server.url.replace(/^http/, 'ws')}/terminals/${encodeURIComponent(terminal.id)}/attach?secret=${encodeURIComponent(server.secret)}`;
      const markers = await page.evaluate(async (url) => {
        const socket = new WebSocket(url);
        socket.binaryType = 'arraybuffer';
        const seen = new Set<number>();
        const arrivals: Marker[] = [];
        let tail = '';
        return await new Promise<Marker[]>((resolve, reject) => {
          socket.addEventListener('open', () => {
            socket.send('GO\n');
          });
          socket.addEventListener('error', () => {
            reject(new Error('Bare PTY socket failed'));
          });
          socket.addEventListener('message', (event) => {
            if (!(event.data instanceof ArrayBuffer)) return;
            const now = performance.now();
            const joined = tail + new TextDecoder().decode(event.data);
            for (const match of joined.matchAll(/FRAME:(\d{4}):(\d{13})/g)) {
              const index = Number(match[1]);
              if (seen.has(index)) continue;
              seen.add(index);
              arrivals.push({
                index,
                processEpochMs: Number(match[2]),
                browserEpochMs: Date.now(),
                browserPerfMs: now,
              });
            }
            tail = joined.slice(-48);
            if (arrivals.length === 600) {
              socket.close();
              resolve(arrivals);
            }
          });
        });
      }, socketUrl);
      const lag = markers.map((marker) => marker.browserEpochMs - marker.processEpochMs);
      const summary = {
        viewport,
        terminalRendererMounted: false,
        markerCount: markers.length,
        processToBrowserMs: { p50: percentile(lag, 0.5), p95: percentile(lag, 0.95) },
        markers,
      };
      mkdirSync('target/performance-traces', { recursive: true });
      const path = `target/performance-traces/hs2-36p1np-${viewport.name}-bare-metrics.json`;
      writeFileSync(path, JSON.stringify(summary, null, 2));
      await testInfo.attach(`${viewport.name}-bare-pty-metrics.json`, {
        path,
        contentType: 'application/json',
      });
      expect(markers).toHaveLength(600);
    } finally {
      await server.stop();
    }
  });
}
