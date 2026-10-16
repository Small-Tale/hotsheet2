import type { Api, ChangeEvent, PollResponse } from './api';

const TICKET_CHANGE_KINDS = new Set([
  'attachment_added',
  'attachment_removed',
  'assigned',
  'changed',
  'claimed',
  'closed',
  'created',
  'deleted',
  'moved',
  'released',
  'renewed',
  'updated',
]);

export interface ProjectChangePollOptions {
  client: Pick<Api, 'pollEvents'>;
  refresh(): Promise<void>;
  beforeRefresh?(): Promise<void>;
  shouldRefresh?(response: PollResponse): boolean;
  onEvents?(response: PollResponse): Promise<void>;
  /**
   * Continuity was (re)established without a replayable gap: the first handshake, a fresh
   * handshake after an outage, or an overflowed replay. Event-driven state that is not
   * ticket rows (pending permissions, drive connections) reconciles here instead of on a
   * timer (HS2-NKCXW4). Never called for a failed request, so an error cannot loop it.
   */
  onResync?(reason: ProjectChangeResync): Promise<void> | void;
  onError?(reason: unknown): void;
  retryMs?: number;
  maxRetryMs?: number;
  wait?: (milliseconds: number, signal: AbortSignal) => Promise<void>;
}

/** Why a change stream asked its owner to reconcile event-driven state. */
export type ProjectChangeResync = 'initial' | 'reconnect' | 'overflow';

export interface ProjectChangeStreamOptions extends Omit<ProjectChangePollOptions, 'client'> {
  client: Pick<Api, 'pollEvents'> & Partial<Pick<Api, 'changeWebSocketUrl'>>;
  openWebSocket?: (url: string) => WebSocket;
  webSocketOpenTimeoutMs?: number;
}

export const containsTicketChange = (response: PollResponse): boolean =>
  response.overflow || response.events.some((event) => TICKET_CHANGE_KINDS.has(event.kind));

export const containsRepositoryChange = (response: PollResponse, checkoutId: string): boolean =>
  response.events.some((event) => event.kind === 'repository_changed' && event.id === checkoutId);

const abortableWait = (milliseconds: number, signal: AbortSignal): Promise<void> =>
  new Promise((resolve) => {
    const timeout = globalThis.setTimeout(resolve, milliseconds);
    signal.addEventListener(
      'abort',
      () => {
        globalThis.clearTimeout(timeout);
        resolve();
      },
      { once: true },
    );
  });

const wasAborted = (signal: AbortSignal): boolean => signal.aborted;

const resync = async (options: ProjectChangePollOptions, reason: ProjectChangeResync) => {
  try {
    await options.onResync?.(reason);
  } catch (error) {
    options.onError?.(error);
  }
};

/**
 * Where a change transport hands what it read (HS2-SH9179). The transport owns the network side:
 * handshake, WebSocket, JSON validation, cursor gap checks, replay, and backoff. The sink owns
 * what the application does with it. Each call is awaited before the transport reads further, so
 * a sink controls back-pressure and per-project ordering.
 */
export interface ProjectChangeSink {
  /** Continuity was (re)established; `cursor` is the replay position at that moment. */
  resync(reason: ProjectChangeResync, cursor: number): Promise<void>;
  /** A replayed or live page; `forceRefresh` reconciles ticket state even without ticket events. */
  deliver(response: PollResponse, forceRefresh: boolean): Promise<void>;
}

export interface ProjectChangeTransportOptions {
  client: Pick<Api, 'pollEvents'> & Partial<Pick<Api, 'changeWebSocketUrl'>>;
  sink: ProjectChangeSink;
  onError?(reason: unknown): void;
  retryMs?: number;
  maxRetryMs?: number;
  wait?: (milliseconds: number, signal: AbortSignal) => Promise<void>;
  openWebSocket?: (url: string) => WebSocket;
  webSocketOpenTimeoutMs?: number;
  /**
   * Resume a stream another owner already followed (a restarted worker, or the main-thread
   * fallback taking over): `cursor` replays from the last acknowledged position, and an
   * undefined cursor performs a fresh handshake that reconciles as a reconnect.
   */
  resume?: { cursor?: number };
}

/**
 * The main-thread half of a change stream: turns transport output into resync, event, barrier,
 * and refresh calls in the same order the original single-function stream made them.
 */
export function createProjectChangeConsumer(options: ProjectChangePollOptions, signal: AbortSignal): ProjectChangeSink {
  return {
    resync: (reason) => resync(options, reason),
    deliver: async (response, forceRefresh) => {
      if (wasAborted(signal)) return;
      if (response.overflow) await resync(options, 'overflow');
      if (wasAborted(signal)) return;
      if (response.events.length && options.onEvents)
        await options.onEvents(response).catch((reason: unknown) => {
          options.onError?.(reason);
        });
      const candidate = forceRefresh || containsTicketChange(response);
      if (!candidate) return;
      await options.beforeRefresh?.().catch((reason: unknown) => {
        options.onError?.(reason);
      });
      if (wasAborted(signal)) return;
      const reconcile = forceRefresh || (options.shouldRefresh?.(response) ?? containsTicketChange(response));
      if (reconcile)
        await options.refresh().catch((reason: unknown) => {
          options.onError?.(reason);
        });
    },
  };
}

/** Run one replay-safe long-poll transport until `signal` aborts. */
export function runProjectChangePollTransport(options: ProjectChangeTransportOptions, signal: AbortSignal): void {
  const wait = options.wait ?? abortableWait;
  const retryMs = options.retryMs ?? 500;
  const maxRetryMs = options.maxRetryMs ?? 30_000;
  const { sink } = options;
  void (async () => {
    let cursor: number | undefined = options.resume?.cursor;
    let reconnecting = options.resume !== undefined && options.resume.cursor === undefined;
    let retryDelay = retryMs;
    for (;;) {
      let response: PollResponse;
      try {
        response = await options.client.pollEvents(cursor, signal);
      } catch (reason) {
        if (wasAborted(signal)) return;
        options.onError?.(reason);
        // A failure is not itself a ticket invalidation. In particular, an older
        // server may not implement polling at all. Remember the outage and
        // reconcile once polling successfully reconnects instead of re-rendering
        // the workspace on every retry.
        reconnecting = true;
        cursor = undefined;
        await wait(retryDelay, signal);
        retryDelay = Math.min(Math.max(retryDelay * 2, retryMs), maxRetryMs);
        if (wasAborted(signal)) return;
        continue;
      }
      if (signal.aborted) return;
      retryDelay = retryMs;
      const handshake = cursor === undefined;
      cursor = response.cursor;
      if (handshake) {
        await sink.resync(reconnecting ? 'reconnect' : 'initial', cursor);
        if (wasAborted(signal)) return;
        // A handshake's own page is not replay: only an outage forces the reconcile.
        if (reconnecting) await sink.deliver({ cursor, events: [], overflow: false }, true);
      } else await sink.deliver(response, false);
      reconnecting = false;
      if (wasAborted(signal)) return;
    }
  })();
}

/** Start one replay-safe long-poll loop. The returned function aborts it permanently. */
export function startProjectChangePoll(
  options: ProjectChangePollOptions & Pick<ProjectChangeTransportOptions, 'resume'>,
): () => void {
  const controller = new AbortController();
  runProjectChangePollTransport(
    { ...options, sink: createProjectChangeConsumer(options, controller.signal) },
    controller.signal,
  );
  return () => {
    controller.abort();
  };
}

const changeEventFromMessage = async (data: unknown): Promise<ChangeEvent> => {
  const text =
    typeof data === 'string'
      ? data
      : data instanceof Blob
        ? await data.text()
        : data instanceof ArrayBuffer
          ? new TextDecoder().decode(data)
          : ArrayBuffer.isView(data)
            ? new TextDecoder().decode(data)
            : '';
  const parsed: unknown = JSON.parse(text);
  if (!parsed || typeof parsed !== 'object') throw new Error('Unsupported project change WebSocket payload.');
  const event = parsed as Partial<ChangeEvent>;
  if (
    typeof event.kind !== 'string' ||
    typeof event.store !== 'string' ||
    typeof event.id !== 'string' ||
    typeof event.slug !== 'string'
  )
    throw new Error('Unsupported project change WebSocket payload.');
  if (event.cursor !== undefined && (!Number.isSafeInteger(event.cursor) || event.cursor < 0))
    throw new Error('Invalid project change WebSocket cursor.');
  return event as ChangeEvent;
};

/**
 * Run the browser's WebSocket-first project change transport until `signal` aborts. A short poll handshake obtains the
 * replay cursor before the socket opens; another zero-wait poll after `open` closes the subscribe
 * race. Disconnects replay through long-poll before a bounded-backoff reconnect, so older servers
 * and environments that reject upgrades retain the complete long-poll behavior.
 */
export function runProjectChangeTransport(options: ProjectChangeTransportOptions, signal: AbortSignal): void {
  if (!options.client.changeWebSocketUrl || (!options.openWebSocket && typeof WebSocket === 'undefined')) {
    runProjectChangePollTransport(options, signal);
    return;
  }
  const controller = { signal },
    { sink } = options,
    wait = options.wait ?? abortableWait,
    retryMs = options.retryMs ?? 500,
    maxRetryMs = options.maxRetryMs ?? 30_000,
    openTimeoutMs = options.webSocketOpenTimeoutMs ?? 2_000;
  const openSocket = options.openWebSocket ?? ((url: string) => new WebSocket(url));

  void (async () => {
    let cursor: number | undefined = options.resume?.cursor,
      reconnecting = options.resume !== undefined,
      retryDelay = retryMs;
    const consume = async (response: PollResponse, forceRefresh = false) => {
      if (wasAborted(controller.signal)) return;
      await sink.deliver(response, forceRefresh);
    };
    const poll = async (since: number | undefined, timeoutMs?: number) =>
      options.client.pollEvents(since, controller.signal, timeoutMs);

    for (;;) {
      if (wasAborted(controller.signal)) return;
      if (cursor === undefined) {
        try {
          const handshake = await poll(undefined);
          if (wasAborted(controller.signal)) return;
          cursor = handshake.cursor;
          await sink.resync(reconnecting ? 'reconnect' : 'initial', cursor);
          if (wasAborted(controller.signal)) return;
          if (reconnecting) await consume(handshake, true);
          reconnecting = false;
          retryDelay = retryMs;
        } catch (reason) {
          if (wasAborted(controller.signal)) return;
          options.onError?.(reason);
          reconnecting = true;
          await wait(retryDelay, controller.signal);
          retryDelay = Math.min(Math.max(retryDelay * 2, retryMs), maxRetryMs);
        }
        continue;
      }

      const outcome = await new Promise<{ reason?: unknown }>((resolve) => {
        let activeSocket: WebSocket | undefined,
          ready = false,
          settled = false,
          failed = false;
        const queued: unknown[] = [];
        let processing = Promise.resolve();
        const finish = (reason?: unknown) => {
          if (settled) return;
          settled = true;
          globalThis.clearTimeout(openTimer);
          controller.signal.removeEventListener('abort', abort);
          void processing.finally(() => {
            resolve(reason ? { reason } : {});
          });
        };
        const fail = (reason: unknown) => {
          if (failed) return;
          failed = true;
          options.onError?.(reason);
          const shouldClose = activeSocket?.readyState === 0 || activeSocket?.readyState === 1;
          finish(reason);
          if (shouldClose) activeSocket?.close();
        };
        const accept = async (raw: unknown) => {
          const event = await changeEventFromMessage(raw);
          if (event.cursor === undefined) {
            if (event.kind !== 'announce') throw new Error('Project change WebSocket event is not replayable.');
            await consume({ cursor: cursor!, events: [event], overflow: false });
            return;
          }
          if (event.cursor <= cursor!) return;
          if (event.cursor !== cursor! + 1)
            throw new Error(`Project change WebSocket cursor jumped from ${cursor} to ${event.cursor}.`);
          cursor = event.cursor;
          retryDelay = retryMs;
          await consume({ cursor, events: [event], overflow: false });
        };
        const enqueue = (raw: unknown) => {
          processing = processing.then(() => accept(raw)).catch(fail);
        };
        const abort = () => {
          activeSocket?.close();
          finish();
        };
        const openTimer = globalThis.setTimeout(() => {
          fail(new Error('Project change WebSocket handshake timed out.'));
        }, openTimeoutMs);
        controller.signal.addEventListener('abort', abort, { once: true });
        try {
          activeSocket = openSocket(options.client.changeWebSocketUrl!());
        } catch (reason) {
          fail(reason);
          return;
        }
        activeSocket.addEventListener(
          'open',
          () => {
            globalThis.clearTimeout(openTimer);
            processing = processing
              .then(async () => {
                const catchup = await poll(cursor, 0);
                cursor = catchup.cursor;
                await consume(catchup);
                ready = true;
                for (const raw of queued.splice(0)) await accept(raw);
              })
              .catch(fail);
          },
          { once: true },
        );
        activeSocket.addEventListener('message', (event) => {
          if (ready) enqueue(event.data);
          else queued.push(event.data);
        });
        activeSocket.addEventListener(
          'error',
          () => {
            fail(new Error('Project change WebSocket failed.'));
          },
          { once: true },
        );
        activeSocket.addEventListener(
          'close',
          () => {
            finish();
          },
          { once: true },
        );
      });
      if (wasAborted(controller.signal)) return;
      reconnecting = true;
      if (!outcome.reason) options.onError?.(new Error('Project change WebSocket closed.'));
      const fallbackStarted = Date.now();
      try {
        // Long-poll through the whole retry window (HS2-BX4MCH). A page that returns early
        // with events is followed by another long poll for the rest of the window, so an
        // event that arrives later in it is delivered at once. Sleeping out the remainder
        // instead delayed it by up to `maxRetryMs`. Only an empty page ends the window early:
        // either the server held the poll for the whole window, or it ignored the timeout. In
        // that case the bounded wait below still rate-limits the retries.
        for (;;) {
          const replay = await poll(cursor, Math.max(0, retryDelay - (Date.now() - fallbackStarted)));
          // The replay cursor is authoritative. An empty gap after a failed upgrade does not
          // invalidate ticket state; forcing a refresh here made every unsupported WebSocket
          // retry wake the entire workspace. Ticket events and overflow still reconcile through
          // consume(), while an actual poll outage falls back to the fresh-handshake path below.
          cursor = replay.cursor;
          await consume(replay);
          if (wasAborted(controller.signal)) return;
          if (replay.events.length === 0 || Date.now() - fallbackStarted >= retryDelay) break;
        }
      } catch (reason) {
        options.onError?.(reason);
        cursor = undefined;
      }
      if (wasAborted(controller.signal)) return;
      const remaining = Math.max(0, retryDelay - (Date.now() - fallbackStarted));
      if (remaining) await wait(remaining, controller.signal);
      retryDelay = Math.min(Math.max(retryDelay * 2, retryMs), maxRetryMs);
    }
  })();
}

/**
 * Start the browser's main-thread WebSocket-first project change stream: the transport above
 * feeding the main-thread consumer. It is the default stream and the fallback whenever the
 * shared change-stream worker is disabled or unavailable (HS2-SH9179).
 */
export function startProjectChangeStream(
  options: ProjectChangeStreamOptions & Pick<ProjectChangeTransportOptions, 'resume'>,
): () => void {
  const controller = new AbortController();
  runProjectChangeTransport(
    { ...options, sink: createProjectChangeConsumer(options, controller.signal) },
    controller.signal,
  );
  return () => {
    controller.abort();
  };
}
