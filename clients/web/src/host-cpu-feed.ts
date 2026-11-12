import { signal } from 'kerfjs';

import { type ResponseParser, ServerResponseShapeError } from './server-response-parsers';

/**
 * The live CPU feed (HS2-CPFBWP, docs/host-health.md "Live CPU"): while the Stats This Mac cards show,
 * the client long-polls `GET /host-health/cpu?after=<seq>`. The server answers as soon as its fast
 * sampler has a newer frame (every two seconds), so the next request is paced by the server, never by a
 * client timer. Hidden Stats issues no request.
 */
export type HostCpuStatus = 'live' | 'starting' | 'unavailable' | 'unsupported';

export interface HostCpuFrame {
  seq: number;
  status: HostCpuStatus;
  sampled_at: string | null;
  busy_percent: number | null;
  /** Each logical core's busy percent over the interval, in OS order. */
  core_percent: number[];
  interval_ms: number;
}

const STATUSES: readonly string[] = ['live', 'starting', 'unavailable', 'unsupported'];
const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value);

export const parseHostCpuFrame: ResponseParser<HostCpuFrame> = (value) => {
  const ok =
    isRecord(value) &&
    Number.isSafeInteger(value.seq) &&
    (value.seq as number) >= 0 &&
    STATUSES.includes(value.status as string) &&
    (value.sampled_at === null || typeof value.sampled_at === 'string') &&
    (value.busy_percent === null || typeof value.busy_percent === 'number') &&
    Array.isArray(value.core_percent) &&
    value.core_percent.every((item) => typeof item === 'number') &&
    typeof value.interval_ms === 'number';
  if (!ok) throw new ServerResponseShapeError('live CPU');
  return value as unknown as HostCpuFrame;
};

export interface HostCpuFeedState {
  frame?: HostCpuFrame;
  /** Set while the last request failed; the next attempt waits `retryMs`. */
  error?: string;
}

type Transport = { hostCpu: (after: number | undefined, signal: AbortSignal) => Promise<HostCpuFrame> };

export interface HostCpuFeedOptions {
  /** First retry delay after a failure; doubles to `maxRetryMs`. */
  retryMs?: number;
  maxRetryMs?: number;
  /** Injected for tests. */
  wait?: (ms: number, signal: AbortSignal) => Promise<void>;
}

const sleep = (ms: number, abort: AbortSignal) =>
  new Promise<void>((resolve) => {
    if (abort.aborted) {
      resolve();
      return;
    }
    const timer = globalThis.setTimeout(resolve, ms);
    abort.addEventListener(
      'abort',
      () => {
        globalThis.clearTimeout(timer);
        resolve();
      },
      { once: true },
    );
  });

/**
 * One server's live CPU feed. `start()` runs the long-poll loop until `stop()`; failures back off
 * from `retryMs` to `maxRetryMs`, and an `unsupported` answer ends the loop, so a platform without a
 * per-core reader costs one request per start.
 */
export function createHostCpuFeed(transport: () => Transport, options: HostCpuFeedOptions = {}) {
  const retryMs = options.retryMs ?? 1_000,
    maxRetryMs = options.maxRetryMs ?? 30_000,
    wait = options.wait ?? sleep;
  const state = signal<HostCpuFeedState>({});
  let controller: AbortController | undefined;

  async function loop(abort: AbortSignal) {
    // Read through a function: an await can abort the signal, which narrowing cannot see.
    const stopped = () => abort.aborted;
    let delay = retryMs;
    while (!stopped()) {
      try {
        const frame = await transport().hostCpu(state.peek().frame?.seq, abort);
        if (stopped()) return;
        state.value = { frame };
        delay = retryMs;
        if (frame.status === 'unsupported') return;
        // A reader failure answers at once; wait like any failure so it cannot spin.
        if (frame.status === 'unavailable') {
          await wait(delay, abort);
          delay = Math.min(delay * 2, maxRetryMs);
        }
      } catch (reason) {
        if (stopped()) return;
        state.value = { ...state.peek(), error: reason instanceof Error ? reason.message : String(reason) };
        await wait(delay, abort);
        delay = Math.min(delay * 2, maxRetryMs);
      }
    }
  }

  /** Begin long-polling; a second start while running is a no-op. */
  function start() {
    if (controller) return;
    controller = new AbortController();
    void loop(controller.signal);
  }

  /** Stop polling and abort the request in flight. The last frame stays for the next start. */
  function stop() {
    controller?.abort();
    controller = undefined;
  }

  return { state, start, stop, running: () => controller !== undefined };
}

export type HostCpuFeed = ReturnType<typeof createHostCpuFeed>;
