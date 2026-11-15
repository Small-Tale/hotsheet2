import type { ChangeEvent } from '../api';
import { setBrowserSessionGrants, stampedBrowserAuthMode } from '../client-origin';

/** The bridge path that answers this page's own session and grants (HS2-85W6BE, HS2-DXM0YN). */
export const BROWSER_SESSION_PATH = '/__hotsheet/remote-auth/me';

/** The change event kind the server pushes when any browser session's grants or validity change. */
export const BROWSER_GRANTS_EVENT = 'browser_grants';

export interface BrowserGrantsDependencies {
  /** Injected for tests; defaults to `fetch`. */
  request?: typeof fetch;
  /** Whether this page runs on a remote-mode host; only those pages have session grants. */
  remoteMode?: () => boolean;
}

/**
 * Keeps this page's session grants current without a reload (HS2-A3TDNH). The server pushes an
 * anonymous `browser_grants` change event whenever a device's grants change or a session ends; this
 * page then re-reads its own `/remote-auth/me`, which stays the one source of truth. A change stream
 * that (re)connects or overflows may have missed an event, so it re-reads too, and so does a gated
 * route that answered 401 or 403. Reads coalesce: an event during a read schedules exactly one more.
 * Nothing runs on a timer.
 */
export function createBrowserGrants(dependencies: BrowserGrantsDependencies = {}) {
  const request = dependencies.request ?? ((input, init) => fetch(input, init)),
    remoteMode =
      dependencies.remoteMode ?? (() => typeof document !== 'undefined' && stampedBrowserAuthMode() === 'remote');
  let reading: Promise<void> | undefined,
    readAgain = false,
    /** The newest grants event cursor acted on; reset on resync because a restarted server restarts its cursors. */
    seen: number | undefined;

  async function readOnce(): Promise<void> {
    try {
      const response = await request(BROWSER_SESSION_PATH, { credentials: 'same-origin' });
      if (response.ok) {
        setBrowserSessionGrants(await response.json().catch(() => undefined));
        return;
      }
      // The session itself ended (revoked or signed out elsewhere): no grant survives it.
      if (response.status === 401) setBrowserSessionGrants(undefined);
      // Any other failure keeps the last known grants; the server still checks every request.
    } catch {
      // Offline: the stream's reconnect resync reads again.
    }
  }

  /** Re-read the grants now, or once more after the read already in flight. */
  function refresh(): Promise<void> {
    if (!remoteMode()) return Promise.resolve();
    if (reading) {
      readAgain = true;
      return reading;
    }
    // Read through a function: an event during the await sets the flag again.
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

  /** Feed change events from any project stream; only a new `browser_grants` event re-reads. The same
   * event seen through several streams of one server reads once. */
  function applyEvents(events: readonly ChangeEvent[]): Promise<void> | undefined {
    let fresh = false;
    for (const event of events) {
      if (event.kind !== BROWSER_GRANTS_EVENT) continue;
      if (event.cursor === undefined || seen === undefined || event.cursor > seen) fresh = true;
      if (event.cursor !== undefined) seen = Math.max(seen ?? event.cursor, event.cursor);
    }
    return fresh ? refresh() : undefined;
  }

  /** A change stream (re)started or overflowed: a grants event may have been missed. */
  function resync(): Promise<void> {
    seen = undefined;
    return refresh();
  }

  return { refresh, applyEvents, resync };
}

export type BrowserGrants = ReturnType<typeof createBrowserGrants>;
