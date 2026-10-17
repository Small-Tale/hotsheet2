import { createChangeStreamSharedHub } from '../../src/change-stream-shared-hub';

/** Browser integration fixture: real SharedWorker/ports and bridge socket, controlled event source. */
const hub = createChangeStreamSharedHub({
  runTransport: (options, signal) => {
    const url = options.client.changeWebSocketUrl?.();
    if (!url || !options.openWebSocket) return;
    const socket = options.openWebSocket(url);
    socket.addEventListener('message', (event) => {
      const change = JSON.parse(String(event.data)) as {
        cursor: number;
        store: string;
        kind: string;
        id: string;
        slug: string;
      };
      void options.sink.deliver({ cursor: change.cursor, overflow: false, events: [change] }, false);
    });
    signal.addEventListener(
      'abort',
      () => {
        socket.close();
      },
      { once: true },
    );
  },
});

const scope = self as unknown as {
  addEventListener(type: 'connect', listener: (event: MessageEvent) => void): void;
};
scope.addEventListener('connect', (event) => {
  hub.connect(event.ports[0]);
});
