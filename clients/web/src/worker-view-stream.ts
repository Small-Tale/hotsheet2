import type { WorkerView } from './worker-view-model';

export interface WorkerViewClient {
  checkoutWorkers: (checkout: string, source: string, after?: string, signal?: AbortSignal) => Promise<WorkerView>;
}

/** One genuine cursor long poll per linked Git source. Failures back off; stopping aborts the open read. */
export function startWorkerViewStream(options: {
  client: WorkerViewClient;
  checkout: string;
  sources: readonly string[];
  onSnapshot: (source: string, view: WorkerView) => void;
  onError: (source: string, error: unknown) => void;
  delay?: (ms: number, signal: AbortSignal) => Promise<void>;
}): () => void {
  const controller = new AbortController();
  const stopped = () => controller.signal.aborted;
  const wait =
    options.delay ??
    ((ms: number, signal: AbortSignal) =>
      new Promise<void>((resolve) => {
        const timer = window.setTimeout(resolve, ms);
        signal.addEventListener(
          'abort',
          () => {
            window.clearTimeout(timer);
            resolve();
          },
          { once: true },
        );
      }));
  for (const source of options.sources)
    void (async () => {
      let cursor: string | undefined;
      let failures = 0;
      while (!stopped()) {
        try {
          const view = await options.client.checkoutWorkers(options.checkout, source, cursor, controller.signal);
          if (stopped()) break;
          cursor = view.cursor;
          failures = 0;
          options.onSnapshot(source, view);
        } catch (error) {
          if (stopped()) break;
          options.onError(source, error);
          cursor = undefined;
          failures += 1;
          await wait(Math.min(30_000, 1_000 * 2 ** Math.min(failures - 1, 5)), controller.signal);
        }
      }
    })();
  return () => {
    controller.abort();
  };
}
