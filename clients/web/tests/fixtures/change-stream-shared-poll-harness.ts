import { createChangeStreamSharedHub } from '../../src/change-stream-shared-hub';

/** A real SharedWorker/MessagePort fixture that drives one poll from each tab. */
const hub = createChangeStreamSharedHub({
  runTransport: (options, signal) => {
    void options.client.pollEvents(undefined, signal, 25_000).then(
      (page) => options.sink.deliver(page, false),
      () => {},
    );
  },
});

const scope = self as unknown as {
  addEventListener(type: 'connect', listener: (event: MessageEvent) => void): void;
};
scope.addEventListener('connect', (event) => {
  hub.connect(event.ports[0]);
});
