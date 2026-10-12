import { createChangeStreamWorkerRuntime } from './change-stream-worker-runtime';

/** Dedicated worker entry for the shared multi-project change stream (HS2-SH9179). */
const scope = self as unknown as {
  postMessage(message: unknown): void;
  addEventListener(type: 'message', listener: (event: MessageEvent) => void): void;
};
const runtime = createChangeStreamWorkerRuntime({
  post: (message) => {
    scope.postMessage(message);
  },
});
scope.addEventListener('message', (event) => {
  runtime.handle(event.data);
});
