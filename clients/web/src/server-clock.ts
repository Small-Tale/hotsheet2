/**
 * Per-project estimate of the server's clock for running-turn timing on remote clients (HS2-GA5ASE).
 *
 * Turn elapsed and last-activity times compare server timestamps (`turn_started_at_ms`,
 * `last_event_at_ms`, `in_flight_tool.started_at_ms`, envelope `at_ms`) with "now". On the server's
 * own machine the browser clock agrees; a phone or LAN client whose clock is minutes off would show
 * wrong durations and call a turn "may be stuck" early or late.
 *
 * Every stamped turn event is a sample: the server stamped it at `at_ms`, no later than the moment it
 * arrived here. Against the browser's monotonic clock, `at_ms - monotonicNow()` is the server-clock
 * anchor less that event's delivery latency, so the largest anchor seen is the best estimate.
 *
 * Why this estimator:
 * - A replayed or delayed event only yields a smaller anchor, so it can never pull the estimate the
 *   wrong way. Before a live event arrives the estimate errs low, which makes durations shorter and
 *   "may be stuck" later, never earlier.
 * - The monotonic base keeps the estimate valid when the browser's wall clock is corrected.
 * - The estimate applies only when it differs from the browser clock by at least the dead zone, so a
 *   same-machine or NTP-synced client keeps its own clock. The estimate is short by the fastest
 *   delivery latency seen, which a slow link, a reconnect backoff, or a loaded host can stretch to
 *   seconds; 30 s keeps that noise out while still correcting the minutes-scale skew that misstates
 *   durations and the 2- and 5-minute "may be stuck" bounds.
 * - No request is made for it: samples ride the change stream the client already receives.
 */
export const SERVER_CLOCK_DEAD_ZONE_MS = 30_000;

export interface ClockSource {
  /** Wall-clock milliseconds since the epoch (`Date.now()`). */
  wallNow(): number;
  /** Monotonic milliseconds (`performance.now()`): unaffected by wall-clock corrections. */
  monotonicNow(): number;
}

const browserClock: ClockSource = {
  wallNow: () => Date.now(),
  monotonicNow: () => performance.now(),
};

export interface ServerClocks {
  /** Record a server timestamp received just now for `projectId`. Non-finite values are ignored. */
  observe(projectId: string, serverAtMs: unknown): void;
  /** The project's server "now" in epoch milliseconds; the browser clock when there is no skew. */
  now(projectId: string): number;
  /** Server minus browser clock for the project, in milliseconds (0 inside the dead zone). */
  offset(projectId: string): number;
}

export function createServerClocks(
  source: ClockSource = browserClock,
  deadZoneMs = SERVER_CLOCK_DEAD_ZONE_MS,
): ServerClocks {
  const anchors = new Map<string, number>();
  const offset = (projectId: string) => {
    const anchor = anchors.get(projectId);
    if (anchor === undefined) return 0;
    const skew = source.monotonicNow() + anchor - source.wallNow();
    return Math.abs(skew) < deadZoneMs ? 0 : skew;
  };
  return {
    observe(projectId, serverAtMs) {
      if (typeof serverAtMs !== 'number' || !Number.isFinite(serverAtMs)) return;
      const anchor = serverAtMs - source.monotonicNow(),
        previous = anchors.get(projectId);
      if (previous === undefined || anchor > previous) anchors.set(projectId, anchor);
    },
    now: (projectId) => source.wallNow() + offset(projectId),
    offset,
  };
}

/** The application's shared estimate: the change stream observes, turn progress reads. */
export const serverClocks = createServerClocks();
