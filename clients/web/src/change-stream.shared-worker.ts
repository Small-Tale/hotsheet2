import { createChangeStreamSharedHub } from './change-stream-shared-hub';

const hub = createChangeStreamSharedHub();
const scope = self as unknown as {
  addEventListener(type: 'connect', listener: (event: MessageEvent) => void): void;
};

scope.addEventListener('connect', (event) => {
  hub.connect(event.ports[0]);
});
setInterval(() => {
  hub.sweep();
}, 30_000);
