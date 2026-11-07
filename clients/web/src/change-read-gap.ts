/**
 * Continuity between a project's ticket reads and its change stream (HS2-SQKJYG).
 *
 * A project's tickets are read over HTTP while its change stream handshakes separately, in either
 * order. The server stamps every authenticated response with the change cursor it was served at
 * (`X-Hotsheet-Change-Cursor`). A read stamped below the stream's first handshake cursor may miss
 * changes in between, which the stream itself never delivers. Instead of reading the collection
 * again, the stream owner replays exactly that gap with one zero-wait poll and reconciles only
 * when it holds a ticket change. An idle start (stamp equal to the handshake) costs nothing.
 */
import type { PollResponse } from './api';

/** The server's response header naming the change cursor a request was served at. */
export const CHANGE_CURSOR_HEADER = 'x-hotsheet-change-cursor';

/** Whether a request reads a ticket collection, whose stamp bounds what the client shows. */
export function isTicketCollectionRead(method: string, path: string): boolean {
  return method.toUpperCase() === 'GET' && path.split('?')[0].endsWith('/tickets');
}

/** A response's change-cursor stamp, or undefined from a server that does not send one. */
export function responseChangeCursor(headers: Pick<Headers, 'get'>): number | undefined {
  const raw = headers.get(CHANGE_CURSOR_HEADER);
  if (raw === null || !/^\d+$/u.test(raw)) return undefined;
  const cursor = Number(raw);
  return Number.isSafeInteger(cursor) ? cursor : undefined;
}

type ReadListener = (cursor: number) => void;

/**
 * Ticket-read stamps per API base. Before a project's stream follows its base, the lowest stamp is
 * kept (the startup load precedes the stream); while followed, each stamp goes to the follower.
 */
export class TicketReadCursors {
  private readonly lowest = new Map<string, number>();
  private readonly followers = new Map<string, Set<ReadListener>>();

  note(base: string, cursor: number): void {
    const followers = this.followers.get(base);
    if (followers?.size) {
      for (const listener of followers) listener(cursor);
      return;
    }
    const previous = this.lowest.get(base);
    if (previous === undefined || cursor < previous) this.lowest.set(base, cursor);
  }

  /** Receive this base's stamps, starting with the lowest one noted before; returns the unfollow. */
  follow(base: string, listener: ReadListener): () => void {
    const earlier = this.lowest.get(base);
    this.lowest.delete(base);
    let followers = this.followers.get(base);
    if (!followers) this.followers.set(base, (followers = new Set()));
    followers.add(listener);
    if (earlier !== undefined) listener(earlier);
    return () => {
      followers.delete(listener);
      if (!followers.size && this.followers.get(base) === followers) this.followers.delete(base);
    };
  }
}

/** The page's shared registry, fed by `Api` and followed by each project's change stream. */
export const ticketReadCursors = new TicketReadCursors();

export interface HandshakeGapOptions {
  /** One zero-wait replay of the change log after `since`. */
  replay(since: number): Promise<PollResponse>;
  /** A gap page (events in the gap only; `overflow` when the gap could not be replayed). */
  reconcile(page: PollResponse): Promise<void>;
  onError?(reason: unknown): void;
}

/**
 * One stream's gap check. `read` takes each ticket-read stamp, `handshake` the stream's first
 * handshake cursor. Every stamp below the cursor covered so far replays the uncovered span once,
 * so repeated or overlapping reads never replay the same events twice.
 */
export function createHandshakeGapReconciler(options: HandshakeGapOptions) {
  let covered: number | undefined,
    lowest: number | undefined,
    stopped = false,
    chain = Promise.resolve();
  // Read through a call: `stop` may run while a replay is awaited.
  const isStopped = () => stopped;
  const check = () => {
    if (stopped || covered === undefined || lowest === undefined || lowest >= covered) return;
    const since = lowest,
      upper = covered;
    covered = since;
    chain = chain.then(async () => {
      if (isStopped()) return;
      let page: PollResponse;
      try {
        const replayed = await options.replay(since);
        page = {
          cursor: upper,
          overflow: replayed.overflow,
          events: replayed.events.filter(
            (event) => event.cursor !== undefined && event.cursor > since && event.cursor <= upper,
          ),
        };
      } catch (reason) {
        options.onError?.(reason);
        // The gap is unknown: reconcile as an overflow rather than risk a lost change.
        page = { cursor: upper, events: [], overflow: true };
      }
      if (isStopped() || (!page.overflow && !page.events.length)) return;
      await options.reconcile(page).catch((reason: unknown) => {
        options.onError?.(reason);
      });
    });
  };
  return {
    read: (cursor: number): void => {
      if (lowest === undefined || cursor < lowest) lowest = cursor;
      check();
    },
    /** The stream's first handshake; later resyncs reconcile on their own. */
    handshake: (cursor: number): void => {
      if (covered !== undefined) return;
      covered = cursor;
      check();
    },
    stop: (): void => {
      stopped = true;
    },
    /** Settles once every started check finished, for tests. */
    settled: (): Promise<void> => chain,
  };
}
