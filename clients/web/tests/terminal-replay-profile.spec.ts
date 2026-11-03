import { mkdirSync, writeFileSync } from 'node:fs';
import { connect as tcpConnect, createServer, type Server, type Socket } from 'node:net';
import { performance } from 'node:perf_hooks';
import { fileURLToPath } from 'node:url';
import { brotliCompressSync, constants as zlibConstants, deflateRawSync } from 'node:zlib';

import { expect, test } from '@playwright/test';
import WebSocket from 'ws';

import { realTicketServer } from './real-ticket-server';

// HS2-K93QHA: an opt-in baseline for cold terminal attach. It measures what one attach costs
// today: the replay bytes the server sends, the time to the first painted frame on a throttled
// phone, the client parse and render time, retained heap, and how well the replay compresses.
// Run `npm run profile:terminal-replay`. It changes no product behavior.
test.skip(process.env.HOTSHEET_TERMINAL_REPLAY_PROFILE !== '1', 'Run npm run profile:terminal-replay.');
test.use({ browserName: 'chromium' });

const MARKER = 'K93QHA-REPLAY-END';
const REPO = fileURLToPath(new URL('../../../', import.meta.url));

// Synthetic output only (customer-data hygiene): this repository's own Git history, a generated
// agent-style transcript with spinner redraws, and a dense worst case.
const GENERATOR = [
  'import os, random, subprocess, sys',
  'kind, target, repo = sys.argv[1], int(sys.argv[2]), sys.argv[3]',
  'out = sys.stdout.buffer',
  'random.seed(7)',
  'written = 0',
  'def emit(data):',
  '    global written',
  '    out.write(data)',
  '    written += len(data)',
  'if kind == "git-log":',
  '    log = subprocess.run(["git", "-C", repo, "log", "--color=always", "--stat", "-n", "4000"], capture_output=True).stdout',
  '    while written < target:',
  // Cut at a line end: a cut inside an SGR sequence would swallow the end marker's first byte.
  '        chunk = log[: target - written + 4096]',
  '        emit(chunk[: chunk.rfind(b"\\n") + 1] or chunk)',
  'elif kind == "agent":',
  '    words = "the replay terminal viewer server attach frame ticket bytes render scroll phone network cursor buffer".split()',
  '    tick = 0',
  '    while written < target:',
  '        emit(b"\\x1b[1;38;5;174m\\xe2\\x97\\x8f\\x1b[0m " + " ".join(random.choice(words) for _ in range(14)).encode() + b"\\n")',
  '        for _ in range(random.randint(2, 6)):',
  '            emit(b"  " + " ".join(random.choice(words) for _ in range(random.randint(8, 18))).encode() + b"\\n")',
  '        emit(b"\\x1b[2m\\xe2\\x94\\x8c\\xe2\\x94\\x80 Bash(cargo nextest run -p hotsheet-terminals)\\x1b[0m\\n")',
  '        for _ in range(random.randint(3, 10)):',
  '            tick += 1',
  '            emit(("\\r\\x1b[2K\\x1b[38;5;174m\\xe2\\x9c\\xbb\\x1b[0m Working\\xe2\\x80\\xa6 (%ds \\xc2\\xb7 \\xe2\\x86\\x91 %d tokens)" % (tick, tick * 37)).encode())',
  '            emit(b"\\x1b[s\\x1b[999;1H\\x1b[2K\\x1b[7m status \\x1b[0m\\x1b[u")',
  '        emit(b"\\r\\x1b[2K\\n")',
  'else:',
  '    alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"',
  '    while written < target:',
  '        emit(bytes(random.choice(alphabet) for _ in range(120)) + b"\\n")',
  'emit(b"\\r\\n' + MARKER + '\\r\\n")',
  'out.flush()',
  'sys.stdin.read()',
].join('\n');

const WORKLOADS = ['git-log', 'agent', 'dense'] as const;
// 256 KiB is the engine floor; 480 000, 960 000 and 3 840 000 are the memory-derived defaults for
// 2 500, 5 000 and 20 000 history lines (HS2-Y5DYC0, `terminal_limits.rs`).
const RING_SIZES = (process.env.HOTSHEET_REPLAY_PROFILE_RINGS ?? '262144,960000,3840000').split(',').map(Number);

interface NetworkProfile {
  name: string;
  downBps: number;
  upBps: number;
  rttMs: number;
}
// Lighthouse's mobile preset (150 ms RTT, 1.6 Mbps down, 750 kbps up) and a typical good LTE link.
const NETWORKS: NetworkProfile[] = [
  { name: 'cellular-slow', downBps: 1_600_000, upBps: 750_000, rttMs: 150 },
  { name: 'cellular-lte', downBps: 9_000_000, upBps: 1_500_000, rttMs: 85 },
];

interface Device {
  name: string;
  width: number;
  height: number;
  cpuSlowdown: number;
  userAgent: string;
}
// A Safari user agent forces the DOM renderer (as on iOS) so the painted rows are observable.
const PHONE: Device = {
  name: 'phone',
  width: 390,
  height: 844,
  cpuSlowdown: 4,
  userAgent:
    'Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Mobile/15E148 Safari/604.1',
};
const DESKTOP: Device = {
  name: 'desktop',
  width: 1440,
  height: 900,
  cpuSlowdown: 1,
  userAgent: 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 Version/18.0 Safari/605.1.15',
};

/** A TCP proxy that serializes each direction at a fixed bandwidth plus half the RTT. */
async function throttledProxy(targetPort: number, network: NetworkProfile) {
  const sockets = new Set<Socket>();
  const pace = (destination: Socket, bitsPerSecond: number) => {
    const queue: { at: number; data: Buffer }[] = [];
    let lineFreeAt = 0;
    let timer: NodeJS.Timeout | undefined;
    let ended = false;
    const flush = () => {
      timer = undefined;
      const now = performance.now();
      while (queue.length > 0 && queue[0].at <= now) destination.write(queue.shift()!.data);
      if (queue.length > 0) timer = setTimeout(flush, queue[0].at - now);
      else if (ended) destination.end();
    };
    return {
      push(chunk: Buffer) {
        for (let offset = 0; offset < chunk.length; offset += 8192) {
          const data = chunk.subarray(offset, offset + 8192);
          const start = Math.max(performance.now(), lineFreeAt);
          lineFreeAt = start + (data.length * 8 * 1000) / bitsPerSecond;
          queue.push({ at: lineFreeAt + network.rttMs / 2, data });
        }
        timer ??= setTimeout(flush, Math.max(0, queue[0].at - performance.now()));
      },
      end() {
        ended = true;
        timer ??= setTimeout(flush, 0);
      },
    };
  };
  const server: Server = createServer((client) => {
    const upstream = tcpConnect(targetPort, '127.0.0.1');
    sockets.add(client).add(upstream);
    const down = pace(client, network.downBps);
    const up = pace(upstream, network.upBps);
    upstream.on('data', (chunk: Buffer) => {
      down.push(chunk);
    });
    client.on('data', (chunk: Buffer) => {
      up.push(chunk);
    });
    upstream.on('end', () => {
      down.end();
    });
    client.on('end', () => {
      up.end();
    });
    for (const socket of [client, upstream])
      socket.on('error', () => {
        client.destroy();
        upstream.destroy();
      });
  });
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve));
  const address = server.address();
  if (address === null || typeof address === 'string') throw new Error('proxy has no port');
  return {
    port: address.port,
    close: async () => {
      for (const socket of sockets) socket.destroy();
      await new Promise<void>((resolve) =>
        server.close(() => {
          resolve();
        }),
      );
    },
  };
}

/**
 * Attach once from Node. Without `waitForMarker` it returns the first binary frame, which is the
 * server's scrollback replay; with it, it waits until the generator has printed its end marker.
 */
async function captureReplay(url: string, waitForMarker: boolean): Promise<Buffer> {
  return await new Promise<Buffer>((resolve, reject) => {
    const socket = new WebSocket(url);
    let received = 0;
    let tail = Buffer.alloc(0);
    const deadline = setTimeout(() => {
      socket.terminate();
      reject(new Error('replay capture timed out after ' + String(received) + ' bytes'));
    }, 180_000);
    socket.on('message', (data, binary) => {
      if (!binary) return;
      const chunk = Buffer.isBuffer(data) ? data : Buffer.from(data as ArrayBuffer);
      received += chunk.length;
      tail = Buffer.concat([tail, chunk]).subarray(-(chunk.length + MARKER.length));
      if (!waitForMarker || tail.includes(MARKER)) {
        clearTimeout(deadline);
        socket.close();
        resolve(chunk);
      }
    });
    socket.on('error', reject);
  });
}

function compression(replay: Buffer) {
  const timed = (label: string, run: () => Buffer) => {
    const start = performance.now();
    const bytes = run().length;
    return {
      label,
      bytes,
      ratio: Number((replay.length / bytes).toFixed(2)),
      ms: Number((performance.now() - start).toFixed(1)),
    };
  };
  return [
    timed('deflate-raw-1', () => deflateRawSync(replay, { level: 1 })),
    timed('deflate-raw-6', () => deflateRawSync(replay, { level: 6 })),
    timed('brotli-5', () => brotliCompressSync(replay, { params: { [zlibConstants.BROTLI_PARAM_QUALITY]: 5 } })),
  ];
}

interface ProfileWindow {
  __replayProfile?: {
    mountAt: number;
    openAt?: number;
    frames: { at: number; bytes: number }[];
    /** The first binary frame, which is always the attach replay (compressed or not). */
    replayAt?: number;
    replayWireBytes?: number;
    header?: string;
    paintedAt?: number;
    /** The first binary frame and preceding header on each socket, by socket index. */
    firstBinaries: { socket: number; at: number; bytes: number; header?: string }[];
  };
  __profileSockets?: InstanceType<typeof globalThis.WebSocket>[];
}

test('profiles cold terminal attach replay (HS2-K93QHA)', async ({ browser }, testInfo) => {
  test.setTimeout(30 * 60_000);
  const server = await realTicketServer();
  const results: Record<string, unknown>[] = [];
  const replays: Record<string, unknown>[] = [];
  try {
    const serverPort = Number(new URL(server.url).port);
    for (const ring of RING_SIZES) {
      await server.request('/terminal-limits', 'PUT', { replay_bytes: ring });
      for (const workload of WORKLOADS) {
        const terminal = await server.request<{ id: string }>('/terminals', 'POST', {
          command: 'python3',
          args: ['-u', '-c', GENERATOR, workload, String(Math.ceil(ring * 1.5)), REPO],
          cwd: server.root,
        });
        const path =
          '/terminals/' + encodeURIComponent(terminal.id) + '/attach?secret=' + encodeURIComponent(server.secret);
        await captureReplay(server.url.replace(/^http/, 'ws') + path, true);
        const replay = await captureReplay(server.url.replace(/^http/, 'ws') + path, false);
        expect(replay.includes(MARKER)).toBe(true);
        replays.push({ ring, workload, replayBytes: replay.length, compression: compression(replay) });
        console.log(JSON.stringify(replays.at(-1)));

        const runs: { device: Device; network?: NetworkProfile }[] = [
          { device: DESKTOP },
          { device: PHONE },
          ...NETWORKS.map((network) => ({ device: PHONE, network })),
        ];
        for (const { device, network } of runs) {
          const proxy = network ? await throttledProxy(serverPort, network) : undefined;
          const context = await browser.newContext({
            viewport: { width: device.width, height: device.height },
            userAgent: device.userAgent,
          });
          const page = await context.newPage();
          try {
            const cdp = await context.newCDPSession(page);
            await page.addInitScript(() => {
              const NativeWebSocket = window.WebSocket;
              window.WebSocket = class extends NativeWebSocket {
                constructor(url: string | URL, protocols?: string | string[]) {
                  super(url, protocols ?? []);
                  const state = (window as ProfileWindow).__replayProfile;
                  if (!state || !String(url).includes('/terminals/')) return;
                  const socket = ((window as ProfileWindow).__profileSockets ??= []).push(this) - 1;
                  let header: string | undefined;
                  this.addEventListener('message', (event: MessageEvent) => {
                    if (typeof event.data === 'string' && event.data.startsWith('{"terminal_frame"'))
                      header ??= event.data;
                    if (!(event.data instanceof ArrayBuffer)) return;
                    if (state.firstBinaries.some((entry) => entry.socket === socket)) return;
                    state.firstBinaries.push({ socket, at: performance.now(), bytes: event.data.byteLength, header });
                  });
                  this.addEventListener('open', () => {
                    state.openAt ??= performance.now();
                  });
                  this.addEventListener('message', (event: MessageEvent) => {
                    if (typeof event.data === 'string' && event.data.startsWith('{"terminal_frame"'))
                      state.header ??= event.data;
                    if (!(event.data instanceof ArrayBuffer)) return;
                    const at = performance.now();
                    state.frames.push({ at, bytes: event.data.byteLength });
                    if (state.replayAt === undefined) {
                      state.replayAt = at;
                      state.replayWireBytes = event.data.byteLength;
                    }
                  });
                }
              };
            });
            await page.goto('/');
            await cdp.send('Emulation.setCPUThrottlingRate', { rate: device.cpuSlowdown });
            await cdp.send('HeapProfiler.collectGarbage');
            const heapBefore = (await cdp.send('Runtime.getHeapUsage')).usedSize;
            const origin = proxy ? 'ws://127.0.0.1:' + String(proxy.port) : server.url.replace(/^http/, 'ws');
            await page.evaluate(
              async ({ url, marker }) => {
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
                const state: NonNullable<ProfileWindow['__replayProfile']> = {
                  mountAt: performance.now(),
                  frames: [],
                  firstBinaries: [],
                };
                (window as ProfileWindow).__replayProfile = state;
                const watch = () => {
                  if (element.querySelector('.xterm-rows')?.textContent.includes(marker)) {
                    state.paintedAt = performance.now();
                    return;
                  }
                  requestAnimationFrame(watch);
                };
                requestAnimationFrame(watch);
                mountTerminalViewport(element, { url });
              },
              { url: origin + path, marker: MARKER },
            );
            await expect
              .poll(() => page.evaluate(() => (window as ProfileWindow).__replayProfile?.paintedAt), {
                timeout: 120_000,
              })
              .toBeGreaterThan(0);
            const state = await page.evaluate(() => (window as ProfileWindow).__replayProfile!);
            const surface = page.locator('.terminal-viewport--dedicated').last();
            const renderer = await surface.getAttribute('data-renderer');
            const scrollbackLimit = await surface.getAttribute('data-scrollback-limit');
            // A reconnect after a dropped socket, still throttled: the viewer reattaches after its
            // first 250 ms backoff and replays (or, with resume, receives only missed output).
            const closedAt = await page.evaluate(() => {
              const sockets = (window as ProfileWindow).__profileSockets ?? [];
              sockets[sockets.length - 1].close();
              return performance.now();
            });
            await expect
              .poll(() => page.evaluate(() => (window as ProfileWindow).__replayProfile!.firstBinaries.length), {
                timeout: 120_000,
              })
              .toBe(2);
            const reconnect = await page.evaluate(() => (window as ProfileWindow).__replayProfile!.firstBinaries[1]);
            await cdp.send('Emulation.setCPUThrottlingRate', { rate: 1 });
            await cdp.send('HeapProfiler.collectGarbage');
            const heapAfter = (await cdp.send('Runtime.getHeapUsage')).usedSize;
            const since = (at: number | undefined) => (at === undefined ? null : Math.round(at - state.mountAt));
            const result = {
              ring,
              workload,
              device: device.name,
              network: network?.name ?? 'loopback',
              renderer,
              scrollbackLimit: Number(scrollbackLimit),
              replayBytes: replay.length,
              framesBeforePaint: state.frames.filter((frame) => frame.at <= state.paintedAt!).length,
              openMs: since(state.openAt),
              replayWireBytes: state.replayWireBytes ?? null,
              replayHeader: state.header ?? null,
              replayReceivedMs: since(state.replayAt),
              paintedMs: since(state.paintedAt),
              clientParseRenderMs: state.replayAt === undefined ? null : Math.round(state.paintedAt! - state.replayAt),
              retainedHeapBytes: heapAfter - heapBefore,
              reconnectMs: Math.round(reconnect.at - closedAt),
              reconnectWireBytes: reconnect.bytes,
              reconnectHeader: reconnect.header ?? null,
            };
            results.push(result);
            console.log(JSON.stringify(result));
            if (ring === RING_SIZES[0] && workload === 'agent' && device === PHONE && network === undefined) {
              mkdirSync('target/performance-traces', { recursive: true });
              const capture = 'target/performance-traces/hs2-k93qha-phone-replay.png';
              writeFileSync(capture, await surface.screenshot({ animations: 'disabled' }));
              await testInfo.attach('phone-replay.png', { path: capture, contentType: 'image/png' });
            }
          } finally {
            await context.close();
            await proxy?.close();
          }
        }
        await server.request('/terminals/' + encodeURIComponent(terminal.id), 'DELETE').catch(() => undefined);
      }
    }
  } finally {
    await server.stop();
  }
  mkdirSync('target/performance-traces', { recursive: true });
  const output = 'target/performance-traces/hs2-k93qha-replay-metrics.json';
  writeFileSync(output, JSON.stringify({ networks: NETWORKS, replays, results }, null, 2));
  await testInfo.attach('replay-metrics.json', { path: output, contentType: 'application/json' });
  console.log(JSON.stringify({ replays, results }));
  expect(results.length).toBe(RING_SIZES.length * WORKLOADS.length * (2 + NETWORKS.length));
});
