import { signal } from 'kerfjs';

import type { ChangeEvent } from './api';
import type { HostHealthSnapshot } from './host-health';

export interface HostHealthResourceState {
  snapshot?: HostHealthSnapshot;
  loading: boolean;
  error?: string;
}

type Transport = { hostHealth: () => Promise<HostHealthSnapshot> };

const message = (reason: unknown) => (reason instanceof Error ? reason.message : String(reason));

/**
 * One server's host health (HS2-DTAVQA). It never polls: it reads `GET /host-health` once on
 * start, again for each `host_health` change event the server pushes, and on change-stream
 * resync (initial, reconnect, overflow). Reads coalesce, so a burst of events, or the same
 * event seen through several project streams of one server, costs at most one extra read.
 */
export function createHostHealthResource(transport: () => Transport) {
  const state = signal<HostHealthResourceState>({ loading: false });
  let reading: Promise<void> | undefined;
  let again = false;
  let disposed = false;
  /** The newest event cursor already acted on; reset on resync because a restarted server restarts its cursors. */
  let seen: number | undefined;

  /** Whether events arrived during the read that just finished. */
  const repeat = () => again && !disposed;

  async function read(): Promise<void> {
    for (;;) {
      again = false;
      state.value = { ...state.peek(), loading: true };
      try {
        const snapshot = await transport().hostHealth();
        if (!disposed) state.value = { snapshot, loading: false };
      } catch (reason) {
        // Keep the last snapshot visible; the error says it may be stale.
        if (!disposed) state.value = { snapshot: state.peek().snapshot, loading: false, error: message(reason) };
      }
      if (!repeat()) return;
    }
  }

  /** Read the snapshot now, or once more after the read already in flight. */
  function refresh(): Promise<void> {
    if (disposed) return Promise.resolve();
    if (reading) {
      again = true;
      return reading;
    }
    reading = read().finally(() => {
      reading = undefined;
    });
    return reading;
  }

  /** Feed change events from any stream of this server; only new `host_health` events trigger a read. */
  function applyEvents(events: readonly ChangeEvent[]): Promise<void> | undefined {
    let fresh = false;
    for (const event of events) {
      if (event.kind !== 'host_health') continue;
      if (event.cursor === undefined || seen === undefined || event.cursor > seen) fresh = true;
      if (event.cursor !== undefined) seen = Math.max(seen ?? event.cursor, event.cursor);
    }
    return fresh ? refresh() : undefined;
  }

  /** The change stream (re)started or overflowed: events may have been missed. */
  function resync(): Promise<void> {
    seen = undefined;
    return refresh();
  }

  function dispose() {
    disposed = true;
  }

  return { state, refresh, applyEvents, resync, dispose };
}
