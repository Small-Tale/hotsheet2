import { createChangePollMultiplexers } from './change-poll-mux';
import { type ChangeStreamWorkerRuntimeOptions, createChangeStreamWorkerRuntime } from './change-stream-worker-runtime';
import { createChangeWebSocketMultiplexers } from './change-ws-mux';
import { parsePollResponse } from './server-response-parsers';

type SharedPort = Pick<MessagePort, 'postMessage' | 'addEventListener' | 'start' | 'close'>;

/** SharedWorker ports own independent transports and delivery queues; their socket and poll pools are shared. */
export function createChangeStreamSharedHub(
  options: Pick<ChangeStreamWorkerRuntimeOptions, 'runTransport' | 'openWebSocket' | 'fetch'> & {
    now?: () => number;
    idleMs?: number;
  } = {},
) {
  const ports = new Map<SharedPort, { dispose(): void; lastSeen: number; noteClientBuild(build: string): void }>();
  // Every port hears the build the shared bridge socket announced, including a port that connects
  // after the announcement (HS2-WHW3M1).
  let clientBuild: string | undefined;
  const webSocketMultiplexerFor = createChangeWebSocketMultiplexers(options.openWebSocket, (build) => {
    clientBuild = build;
    for (const owner of ports.values()) owner.noteClientBuild(build);
  });
  const pollMultiplexerFor = createChangePollMultiplexers({
    parse: parsePollResponse,
    sharedAcrossClients: true,
    ...(options.fetch ? { fetch: options.fetch } : {}),
  });
  const directPollMultiplexerFor = createChangePollMultiplexers({
    parse: parsePollResponse,
    sharedAcrossClients: true,
    directOnly: true,
    ...(options.fetch ? { fetch: options.fetch } : {}),
  });
  const now = options.now ?? Date.now;
  const idleMs = options.idleMs ?? 90_000;
  return {
    connect(port: SharedPort): void {
      const runtime = createChangeStreamWorkerRuntime({
        post: (message) => {
          port.postMessage(message);
        },
        webSocketMultiplexerFor,
        pollMultiplexerFor,
        directPollMultiplexerFor,
        ...(options.fetch ? { fetch: options.fetch } : {}),
        ...(options.runTransport ? { runTransport: options.runTransport } : {}),
      });
      const owner = {
        dispose: () => {
          runtime.dispose();
        },
        lastSeen: now(),
        noteClientBuild: runtime.noteClientBuild,
      };
      if (clientBuild !== undefined) runtime.noteClientBuild(clientBuild);
      ports.set(port, owner);
      port.addEventListener('message', (event) => {
        owner.lastSeen = now();
        const data: unknown = event.data;
        const type = typeof data === 'object' && data !== null && 'type' in data ? data.type : undefined;
        if (type === 'dispose') {
          owner.dispose();
          port.close();
          ports.delete(port);
          return;
        }
        if (type === 'heartbeat') {
          port.postMessage({ type: 'heartbeat' });
          return;
        }
        runtime.handle(data);
      });
      port.start();
    },
    sweep(): void {
      for (const [port, owner] of ports) {
        if (now() - owner.lastSeen <= idleMs) continue;
        owner.dispose();
        port.close();
        ports.delete(port);
      }
    },
    portCount: () => ports.size,
  };
}
