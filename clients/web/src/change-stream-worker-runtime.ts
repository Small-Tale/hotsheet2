import type { Api } from './api';
import { changeMuxTarget, createChangePollMultiplexers } from './change-poll-mux';
import {
  CHANGE_STREAM_CLIENT_BUILD_VERSION,
  CHANGE_STREAM_WORKER_PROTOCOL,
  type ChangeStreamDelivery,
  ChangeStreamDeliveryQueue,
  type ChangeStreamWorkerMessage,
  type ChangeStreamWorkerOpen,
  parseChangeStreamMainMessage,
} from './change-stream-worker-protocol';
import { createChangeWebSocketMultiplexers } from './change-ws-mux';
import { type ProjectChangeTransportOptions, runProjectChangeTransport } from './project-change-poll';
import { parsePollResponse } from './server-response-parsers';

type ChangeClient = ProjectChangeTransportOptions['client'];

export interface ChangeStreamWorkerRuntimeOptions {
  post(message: ChangeStreamWorkerMessage): void;
  fetch?: typeof fetch;
  /** Injected for tests; defaults to the real WebSocket-first transport. */
  runTransport?: typeof runProjectChangeTransport;
  openWebSocket?: (url: string) => WebSocket;
  /** Shared across SharedWorker ports so their long polls occupy one browser request. */
  pollMultiplexerFor?: ReturnType<typeof createChangePollMultiplexers>;
  /** Bounded direct fallback, shared across SharedWorker ports when supplied. */
  directPollMultiplexerFor?: ReturnType<typeof createChangePollMultiplexers>;
  /**
   * Shared across SharedWorker ports so their project sockets use one browser bridge socket. Its
   * owner relays the bridge's client build announcement through `noteClientBuild`.
   */
  webSocketMultiplexerFor?: ReturnType<typeof createChangeWebSocketMultiplexers>;
  maxQueuedEvents?: number;
}

/**
 * The worker-side long-poll/WebSocket client. It reproduces `Api.pollEvents` without the main
 * thread's busy indicator or DOM-bound modules, and validates every page with the same parser.
 */
export function createWorkerChangeClient(open: ChangeStreamWorkerOpen, fetcher: typeof fetch): ChangeClient {
  return {
    pollEvents: async (since, signal, timeoutMs) => {
      const query = `timeout_ms=${timeoutMs ?? 25_000}${since === undefined ? '' : `&since=${since}`}&client=${encodeURIComponent(open.clientId)}`;
      const response = await fetcher(`${open.apiUrl}/ws/poll?${query}`, {
        headers: { 'Content-Type': 'application/json' },
        signal,
      });
      if (!response.ok) throw new Error(`Change poll failed with ${response.status}.`);
      return parsePollResponse(await response.json());
    },
    ...(open.webSocketUrl ? { changeWebSocketUrl: () => open.webSocketUrl! } : {}),
  } satisfies Pick<Api, 'pollEvents'> & Partial<Pick<Api, 'changeWebSocketUrl'>>;
}

/**
 * Everything the dedicated change-stream worker does, independent of the worker global so it can
 * be driven directly in unit tests (HS2-SH9179). One transport per open project; each project has
 * its own bounded delivery queue, so a busy or bursting project never blocks another one.
 */
export function createChangeStreamWorkerRuntime(options: ChangeStreamWorkerRuntimeOptions) {
  const fetcher = options.fetch ?? ((input, init) => fetch(input, init));
  const run = options.runTransport ?? runProjectChangeTransport;
  // One shared long-poll per bridge for every project it serves (HS2-6W1NNF).
  const multiplexerFor =
    options.pollMultiplexerFor ??
    createChangePollMultiplexers({
      parse: parsePollResponse,
      fetch: fetcher,
    });
  const directPollMultiplexerFor =
    options.directPollMultiplexerFor ??
    createChangePollMultiplexers({
      parse: parsePollResponse,
      fetch: fetcher,
      directOnly: true,
    });
  // The page shows the newer-client notice from the build the bridge announces (HS2-WHW3M1). The
  // relay starts after the page's hello, so it always follows `ready`, and repeats only on a change.
  let clientBuild: string | undefined,
    relayedBuild: string | undefined,
    greeted = false;
  const relayClientBuild = () => {
    if (!greeted || clientBuild === undefined || clientBuild === relayedBuild) return;
    relayedBuild = clientBuild;
    options.post({ type: 'client-build', version: CHANGE_STREAM_CLIENT_BUILD_VERSION, build: clientBuild });
  };
  const noteClientBuild = (build: string) => {
    clientBuild = build;
    relayClientBuild();
  };
  const webSocketMultiplexerFor =
    options.webSocketMultiplexerFor ?? createChangeWebSocketMultiplexers(options.openWebSocket, noteClientBuild);
  const changeClient = (message: ChangeStreamWorkerOpen): ChangeClient => {
    const direct = createWorkerChangeClient(message, fetcher);
    const project = changeMuxTarget(new URL(message.apiUrl).pathname, '')?.project ?? message.project;
    if (!message.changeMuxUrl)
      return directPollMultiplexerFor(new URL(message.apiUrl).origin, message.clientId).client(
        project,
        direct,
        message.clientId,
      );
    return multiplexerFor(message.changeMuxUrl, message.clientId).client(project, direct, message.clientId);
  };
  const subscriptions = new Map<
    string,
    { generation: number; controller: AbortController; queue: ChangeStreamDeliveryQueue }
  >();

  const close = (project: string, generation?: number) => {
    const current = subscriptions.get(project);
    if (!current || (generation !== undefined && current.generation !== generation)) return;
    current.controller.abort();
    subscriptions.delete(project);
  };

  const open = (message: ChangeStreamWorkerOpen) => {
    // A newer generation replaces whatever the project had; an older one is a stale reopen.
    const existing = subscriptions.get(message.project);
    if (existing && existing.generation >= message.generation) return;
    close(message.project);
    const controller = new AbortController();
    const { project, generation } = message;
    const queue = new ChangeStreamDeliveryQueue((seq, delivery: ChangeStreamDelivery) => {
      if (!controller.signal.aborted) options.post({ type: 'batch', project, generation, seq, ...delivery });
    }, options.maxQueuedEvents);
    subscriptions.set(project, { generation, controller, queue });
    run(
      {
        client: changeClient(message),
        ...(message.changeMuxUrl && message.webSocketUrl
          ? {
              openWebSocket: (url: string) =>
                webSocketMultiplexerFor(message.changeMuxUrl!, message.clientId).open(
                  message.project,
                  url,
                  message.clientId,
                ),
            }
          : options.openWebSocket
            ? { openWebSocket: options.openWebSocket }
            : {}),
        ...(message.resume ? { resume: message.resume } : {}),
        onError: (reason) => {
          if (controller.signal.aborted) return;
          options.post({
            type: 'error',
            project,
            generation,
            message: reason instanceof Error ? reason.message : String(reason),
          });
        },
        sink: {
          // The transport never waits on the main thread: the queue owns back-pressure, so a slow
          // render cannot stall WebSocket reads or delay another project's delivery.
          resync: (reason, cursor) => {
            queue.push({ kind: 'resync', reason, cursor });
            return Promise.resolve();
          },
          deliver: (response, force) => {
            queue.push({ kind: 'deliver', response, force });
            return Promise.resolve();
          },
        },
      },
      controller.signal,
    );
  };

  return {
    handle(raw: unknown): void {
      const message = parseChangeStreamMainMessage(raw);
      if (!message) return;
      if (message.type === 'hello') {
        options.post({ type: 'ready', protocol: CHANGE_STREAM_WORKER_PROTOCOL });
        greeted = true;
        relayedBuild = undefined;
        relayClientBuild();
      } else if (message.type === 'open') open(message);
      else if (message.type === 'close') close(message.project, message.generation);
      else {
        const current = subscriptions.get(message.project);
        if (current?.generation === message.generation) current.queue.ack(message.seq);
      }
    },
    /** Record the bridge's announced client build and relay it to the page when it changed. */
    noteClientBuild,
    /** Open project ids, for tests. */
    projects: () => [...subscriptions.keys()],
    dispose(): void {
      for (const project of [...subscriptions.keys()]) close(project);
    },
  };
}
