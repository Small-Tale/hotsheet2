/**
 * One cursor long poll over a versioned read (HS2-J89JC7, HS2-S3FKJ7). Each read passes the version it
 * last saw, so the server answers when what it shows changes; a new version is reported once. Failures
 * forget the version and back off (1 s doubling to 30 s); stopping aborts the open read. No timer reads.
 */
export function startVersionedLongPoll<T extends { version: string }>(options: {
  read: (after: string | undefined, signal: AbortSignal) => Promise<T>;
  onValue: (value: T) => void;
  onError: (error: unknown) => void;
  delay?: (ms: number, signal: AbortSignal) => Promise<void>;
}): () => void {
  const controller = new AbortController();
  const stopped = () => controller.signal.aborted;
  const wait =
    options.delay ??
    ((ms: number, abort: AbortSignal) =>
      new Promise<void>((resolve) => {
        const timer = window.setTimeout(resolve, ms);
        abort.addEventListener(
          'abort',
          () => {
            window.clearTimeout(timer);
            resolve();
          },
          { once: true },
        );
      }));
  void (async () => {
    let version: string | undefined;
    let failures = 0;
    while (!stopped()) {
      try {
        const value = await options.read(version, controller.signal);
        if (stopped()) break;
        failures = 0;
        if (value.version !== version) options.onValue(value);
        version = value.version;
      } catch (error) {
        if (stopped()) break;
        options.onError(error);
        version = undefined;
        failures += 1;
        await wait(Math.min(30_000, 1_000 * 2 ** Math.min(failures - 1, 5)), controller.signal);
      }
    }
  })();
  return () => {
    controller.abort();
  };
}
