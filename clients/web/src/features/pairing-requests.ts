import { signal } from 'kerfjs';

import type { ChangeEvent } from '../api';
import { canManageRemoteDevices } from '../client-origin';
import { parseRemoteDeviceList, type RemotePendingDevice } from '../remote-devices-model';
import { REMOTE_DEVICES_PATH } from './remote-devices';

/** The change event kind the server pushes, to device managers only, when the requests waiting for
 * approval change (HS2-BZHMEV). It names no request. */
export const PAIRING_REQUESTS_EVENT = 'pairing_requests';

export interface PairingRequestsDependencies {
  /** Injected for tests; defaults to `fetch`. */
  request?: typeof fetch;
  /** Whether this page may manage devices; defaults to the session grant (HS2-DXM0YN). */
  canManage?: () => boolean;
  /** A request that was not waiting before arrived after the first read (HS2-BZHMEV). */
  onArrival?: (request: RemotePendingDevice) => void;
  /** The device-list read answered 401 or 403: this page's grants may be stale (HS2-A3TDNH). */
  onForbidden?: () => void;
  /** Wall-clock seconds; injected for tests. */
  now?: () => number;
  /** Local display timer that drops a request when it expires. It never issues a request. */
  schedule?: (callback: () => void, ms: number) => () => void;
}

function scheduleTimeout(callback: () => void, ms: number): () => void {
  const timer = setTimeout(callback, ms);
  return () => {
    clearTimeout(timer);
  };
}

/**
 * The pairing requests waiting for this host's approval, for an admin or host page that has not opened
 * App Settings → Devices (HS2-BZHMEV). The server pushes an anonymous `pairing_requests` change event,
 * only to device managers, when a request arrives or is approved or rejected; this page then re-reads
 * the admin-gated device list once. Every change-stream resync re-reads too, reads coalesce, and the
 * open Devices list feeds its own updates in. Nothing polls: an expiry is dropped by a local timer
 * that issues no request. A page that may not manage devices reads nothing and shows no requests.
 */
export function createPairingRequests(dependencies: PairingRequestsDependencies = {}) {
  const request = dependencies.request ?? ((input, init) => fetch(input, init)),
    canManage = dependencies.canManage ?? (() => canManageRemoteDevices()),
    now = dependencies.now ?? (() => Date.now() / 1000),
    schedule = dependencies.schedule ?? scheduleTimeout;
  /** Requests still waiting for approval, oldest first. Approved-but-unredeemed ones need no action. */
  const waiting = signal<readonly RemotePendingDevice[]>([]);
  let reading: Promise<void> | undefined,
    readAgain = false,
    /** The newest pairing event cursor acted on; reset on resync because a restarted server restarts it. */
    seen: number | undefined,
    /** Request ids already shown, so a re-read never announces one twice. Undefined before the first read. */
    known: Set<string> | undefined,
    /** A host without remote authentication answers 503; it pushes no events, so resyncs stop asking. */
    unconfigured = false,
    cancelExpiry: (() => void) | undefined;

  /** Show `pending`'s waiting requests, announce new ones, and arm the next expiry. */
  function show(pending: readonly RemotePendingDevice[]) {
    const at = now(),
      next = pending.filter((item) => !item.approved && item.expires_at > at);
    if (known) for (const item of next) if (!known.has(item.id)) dependencies.onArrival?.(item);
    known = new Set([...(known ?? []), ...next.map((item) => item.id)]);
    const current = waiting.peek();
    if (current.length !== next.length || current.some((item, index) => item.id !== next[index]?.id))
      waiting.value = next;
    cancelExpiry?.();
    cancelExpiry = undefined;
    if (next.length === 0) return;
    const soonest = Math.min(...next.map((item) => item.expires_at));
    cancelExpiry = schedule(
      () => {
        show(waiting.peek());
      },
      Math.max(0, (soonest - now()) * 1000),
    );
  }

  /** Forget every request: this page may no longer see them. */
  function clear() {
    cancelExpiry?.();
    cancelExpiry = undefined;
    known = undefined;
    if (waiting.peek().length > 0) waiting.value = [];
  }

  async function readOnce(): Promise<void> {
    try {
      const response = await request(REMOTE_DEVICES_PATH, { credentials: 'same-origin' });
      if (response.status === 401 || response.status === 403) {
        clear();
        dependencies.onForbidden?.();
        return;
      }
      if (response.status === 503) {
        unconfigured = true;
        clear();
        return;
      }
      if (!response.ok) return;
      unconfigured = false;
      const list = parseRemoteDeviceList(await response.json());
      // The grant may have gone while the read was in flight.
      if (canManage()) show(list.pending);
      else clear();
    } catch {
      // Offline or an invalid answer: keep the last known requests; the next resync reads again.
    }
  }

  /** Re-read the waiting requests now, or once more after the read in flight. A page that may not manage
   * devices reads nothing and drops what it showed. */
  function refresh(): Promise<void> {
    if (!canManage()) {
      clear();
      return Promise.resolve();
    }
    if (reading) {
      readAgain = true;
      return reading;
    }
    const again = () => readAgain;
    reading = (async () => {
      for (;;) {
        readAgain = false;
        await readOnce();
        if (!again()) return;
      }
    })().finally(() => {
      reading = undefined;
    });
    return reading;
  }

  /** Feed change events from any project stream; only a new `pairing_requests` event re-reads. The same
   * event seen through several streams of one server reads once. */
  function applyEvents(events: readonly ChangeEvent[]): Promise<void> | undefined {
    let fresh = false;
    for (const event of events) {
      if (event.kind !== PAIRING_REQUESTS_EVENT) continue;
      if (event.cursor === undefined || seen === undefined || event.cursor > seen) fresh = true;
      if (event.cursor !== undefined) seen = Math.max(seen ?? event.cursor, event.cursor);
    }
    if (!fresh) return undefined;
    unconfigured = false;
    return refresh();
  }

  /** A change stream (re)started or overflowed: a pairing event may have been missed. */
  function resync(): Promise<void> {
    seen = undefined;
    if (unconfigured) return Promise.resolve();
    return refresh();
  }

  /** The open Devices list reported the host's requests; show them without another read. */
  function observe(pending: readonly RemotePendingDevice[]) {
    if (canManage()) show(pending);
  }

  return { waiting, refresh, applyEvents, resync, observe, clear };
}

export type PairingRequests = ReturnType<typeof createPairingRequests>;

/** The transient notice for one new request; the Settings and Devices badges stay as its record. */
export function pairingRequestNotice(request: Pick<RemotePendingDevice, 'label'>): string {
  return `“${request.label}” asks to pair with this host. Open Settings → Devices to approve or reject it.`;
}
