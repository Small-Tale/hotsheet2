import type { Api, PollResponse } from './api';

const TICKET_CHANGE_KINDS = new Set([
  'attachment_added', 'attachment_removed', 'assigned', 'changed', 'claimed',
  'closed', 'created', 'deleted', 'moved', 'released', 'renewed', 'updated',
]);

export interface ProjectChangePollOptions {
  client: Pick<Api, 'pollEvents'>;
  refresh(): Promise<void>;
  onError?(reason: unknown): void;
  retryMs?: number;
  maxRetryMs?: number;
  wait?: (milliseconds: number, signal: AbortSignal) => Promise<void>;
}

export const containsTicketChange = (response: PollResponse): boolean =>
  response.overflow || response.events.some(event => TICKET_CHANGE_KINDS.has(event.kind));

const abortableWait = (milliseconds: number, signal: AbortSignal): Promise<void> => new Promise(resolve => {
  const timeout = window.setTimeout(resolve, milliseconds);
  signal.addEventListener('abort', () => { window.clearTimeout(timeout); resolve(); }, { once: true });
});

const wasAborted = (signal: AbortSignal): boolean => signal.aborted;

/** Start one replay-safe long-poll loop. The returned function aborts it permanently. */
export function startProjectChangePoll(options: ProjectChangePollOptions): () => void {
  const controller = new AbortController();
  const wait = options.wait ?? abortableWait;
  const retryMs = options.retryMs ?? 500;
  const maxRetryMs = options.maxRetryMs ?? 30_000;
  void (async () => {
    let cursor: number | undefined;
    let reconnecting = false;
    let retryDelay = retryMs;
    for (;;) {
      let response: PollResponse;
      try {
        response = await options.client.pollEvents(cursor, controller.signal);
      } catch (reason) {
        if (controller.signal.aborted) return;
        options.onError?.(reason);
        // A failure is not itself a ticket invalidation. In particular, an older
        // server may not implement polling at all. Remember the outage and
        // reconcile once polling successfully reconnects instead of re-rendering
        // the workspace on every retry.
        reconnecting = true;
        cursor = undefined;
        await wait(retryDelay, controller.signal);
        retryDelay = Math.min(Math.max(retryDelay * 2, retryMs), maxRetryMs);
        if (wasAborted(controller.signal)) return;
        continue;
      }
      if (controller.signal.aborted) return;
      retryDelay = retryMs;
      const handshake = cursor === undefined;
      cursor = response.cursor;
      const reconcile = (handshake && reconnecting) || (!handshake && containsTicketChange(response));
      reconnecting = false;
      if (reconcile) await options.refresh().catch((reason: unknown) => { options.onError?.(reason); });
    }
  })();
  return () => { controller.abort(); };
}
