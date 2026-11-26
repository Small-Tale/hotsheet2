/**
 * The client's bounded failure backoff (HS2-JGFPG4), shared by the cursor long polls
 * (`versioned-long-poll.ts`, `worker-view-stream.ts`) and the one-shot reads that retry a transient
 * failure. Waits are 1 s doubling to 30 s and end early when their signal aborts; nothing here runs
 * on a fixed interval or at import time.
 */
import { ApiHttpError } from './api';

/** The wait before the next attempt after `failures` consecutive failures (1 s doubling to 30 s). */
export const boundedBackoffMs = (failures: number): number =>
  Math.min(30_000, 1_000 * 2 ** Math.min(Math.max(failures, 1) - 1, 5));

/** A wait that resolves after `ms`, or at once when `signal` aborts. */
export type BackoffDelay = (ms: number, signal: AbortSignal) => Promise<void>;

export const abortableDelay: BackoffDelay = (ms, signal) =>
  new Promise<void>((resolve) => {
    if (signal.aborted) {
      resolve();
      return;
    }
    const timer = window.setTimeout(resolve, ms);
    signal.addEventListener(
      'abort',
      () => {
        window.clearTimeout(timer);
        resolve();
      },
      { once: true },
    );
  });

/**
 * Whether a failed request may succeed if asked again: the network or the server failed (no answer,
 * a 5xx, 408 or 429). Any other refusal is the server's answer and asking again does not change it.
 */
export function isTransientRequestFailure(reason: unknown): boolean {
  if (!(reason instanceof ApiHttpError)) return true;
  return reason.status >= 500 || reason.status === 408 || reason.status === 429;
}

/** HS2-1HGTD7: a request that got no answer, in words that read the same in every browser. */
export const SERVER_UNREACHABLE = 'The Hot Sheet server could not be reached.';

/**
 * A failed request in plain words. A request that got no answer rejects with the browser's own
 * `TypeError` text ("Failed to fetch", "Load failed"), so it reads as {@link SERVER_UNREACHABLE};
 * the server's own refusal keeps its message.
 */
export function describeRequestFailure(reason: unknown): string {
  if (reason instanceof TypeError) return SERVER_UNREACHABLE;
  return reason instanceof Error ? reason.message : String(reason);
}

/** How many consecutive transient failures a one-shot read retries before it reports the failure. */
export const TRANSIENT_RETRY_LIMIT = 5;

/**
 * Run `attempt`, retrying transient failures with the bounded backoff. It throws the failure when it
 * is not transient, after `maxFailures` consecutive failures, or when `signal` aborts or `wanted()`
 * turns false (the caller no longer needs the answer; the caller tells these apart by checking them).
 */
export async function retryTransient<T>(
  attempt: () => Promise<T>,
  options: {
    signal: AbortSignal;
    delay?: BackoffDelay;
    maxFailures?: number;
    wanted?: () => boolean;
    /** Which failures to retry; `isTransientRequestFailure` by default. */
    transient?: (reason: unknown) => boolean;
  },
): Promise<T> {
  const wait = options.delay ?? abortableDelay,
    limit = options.maxFailures ?? TRANSIENT_RETRY_LIMIT,
    wanted = () => !options.signal.aborted && (options.wanted?.() ?? true),
    transient = options.transient ?? isTransientRequestFailure;
  for (let failures = 1; ; failures += 1) {
    try {
      return await attempt();
    } catch (reason) {
      if (!wanted() || !transient(reason) || failures >= limit) throw reason;
      await wait(boundedBackoffMs(failures), options.signal);
      if (!wanted()) throw reason;
    }
  }
}
