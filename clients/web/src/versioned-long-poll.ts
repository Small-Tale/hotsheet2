import { abortableDelay, boundedBackoffMs } from './bounded-backoff';

/**
 * One cursor long poll over a versioned read (HS2-J89JC7, HS2-S3FKJ7). Each read passes the version it
 * last saw, so the server answers when what it shows changes; a new version is reported once. Failures
 * forget the version and back off (1 s doubling to 30 s); stopping aborts the open read. No timer reads.
 *
 * HS2-YA6D31: an unchanged answer that came back before `EARLY_ANSWER_FLOOR_MS` (an older server, a
 * proxy that ends the hold, a zero server timeout) did not hold, so the next read would follow at
 * once in a tight loop. It takes the same backoff as a failure but keeps the version. A changed
 * answer, or an unchanged one that held at least the floor, resets the backoff.
 */
export const EARLY_ANSWER_FLOOR_MS = 1_000;

/** The wait after the `misses`-th consecutive failure or early unchanged answer: 1 s doubling to 30 s,
 * the client's shared bounded backoff (HS2-JGFPG4). */
export function versionedLongPollBackoff(misses: number): number {
  return boundedBackoffMs(misses);
}

export function startVersionedLongPoll<T extends { version: string }>(options: {
  read: (after: string | undefined, signal: AbortSignal) => Promise<T>;
  onValue: (value: T) => void;
  onError: (error: unknown) => void;
  delay?: (ms: number, signal: AbortSignal) => Promise<void>;
  now?: () => number;
}): () => void {
  const now = options.now ?? (() => Date.now());
  const controller = new AbortController();
  const stopped = () => controller.signal.aborted;
  const wait = options.delay ?? abortableDelay;
  void (async () => {
    let version: string | undefined;
    let misses = 0;
    while (!stopped()) {
      const started = now();
      let early: boolean;
      try {
        const value = await options.read(version, controller.signal);
        if (stopped()) break;
        const changed = value.version !== version;
        early = !changed && now() - started < EARLY_ANSWER_FLOOR_MS;
        if (changed) options.onValue(value);
        version = value.version;
      } catch (error) {
        if (stopped()) break;
        options.onError(error);
        version = undefined;
        early = true;
      }
      if (!early) {
        misses = 0;
        continue;
      }
      misses += 1;
      await wait(versionedLongPollBackoff(misses), controller.signal);
    }
  })();
  return () => {
    controller.abort();
  };
}
