import type { HistoryState, LiveHistorySegment, LiveHistoryStore, LiveStoreState } from './api';

/**
 * The Stats page's Overnight timeline (HS2-9AN4DN, docs/34-glance-surfaces.md §34.8, mock
 * `m3-stats-right-now`): one swimlane per project from the persisted live state transitions
 * (`GET /live-history`), extended live by the pushed snapshots, with a tick per completed ticket.
 */

/** The legend order of the mock: Active, Waiting on CI, May be stuck, Waiting on you, Stopped, then the problems. */
export const HISTORY_STATES: readonly HistoryState[] = [
  'active',
  'waiting_on_ci',
  'may_be_stuck',
  'waiting_on_you',
  'stopped',
  'stalled',
  'halted',
];

export const HISTORY_STATE_LABELS: Readonly<Record<HistoryState, string>> = {
  active: 'Active',
  waiting_on_ci: 'Waiting on CI',
  may_be_stuck: 'May be stuck',
  waiting_on_you: 'Waiting on you',
  stopped: 'Stopped',
  stalled: 'Stalled',
  halted: 'Halted',
};

/** A pushed snapshot's timeline state: the rule the server records (Waiting on CI while every claim is final-testing). */
export function liveHistoryState(state: LiveStoreState): HistoryState {
  if (
    state.state === 'active' &&
    state.claims.length > 0 &&
    state.claims.every((claim) => claim.phase === 'final_testing')
  )
    return 'waiting_on_ci';
  return state.state;
}

/**
 * Extend a store's segments, read at `readAtMs`, to `nowMs` with what the pushed snapshot says now.
 * The server ends the last segment at its own "now", so it is the current state: it grows while the
 * state holds (a small clock skew included), and a different state starts a new segment where it ended.
 */
export function extendSegments(
  segments: readonly LiveHistorySegment[],
  current: LiveStoreState | undefined,
  readAtMs: number,
  nowMs: number,
): LiveHistorySegment[] {
  const out = segments.map((segment) => ({ ...segment }));
  const tail = out.at(-1);
  const state = current ? liveHistoryState(current) : tail?.state;
  if (tail && tail.state === state) {
    tail.end_ms = Math.max(tail.end_ms, nowMs);
    return out;
  }
  if (!state) return out;
  const start = Math.min(nowMs, tail?.end_ms ?? readAtMs);
  if (nowMs > start) out.push({ state, start_ms: start, end_ms: nowMs });
  return out;
}

export interface TimelineSegment extends LiveHistorySegment {
  /** Start and width as fractions of the window (0–1). */
  left: number;
  width: number;
}

export interface TimelineLane {
  store: string;
  name: string;
  segments: TimelineSegment[];
  ticks: { slug: string; at_ms: number; left: number }[];
}

/** Place segments and ticks on `[fromMs, toMs]` as fractions, clipped to the window. */
export function timelineLane(
  store: Pick<LiveHistoryStore, 'store' | 'name' | 'completions'>,
  segments: readonly LiveHistorySegment[],
  fromMs: number,
  toMs: number,
): TimelineLane {
  const span = Math.max(1, toMs - fromMs);
  const position = (ms: number) => Math.min(1, Math.max(0, (ms - fromMs) / span));
  return {
    store: store.store,
    name: store.name,
    segments: segments
      .filter((segment) => segment.end_ms > fromMs && segment.start_ms < toMs)
      .map((segment) => {
        const left = position(segment.start_ms);
        return { ...segment, left, width: position(segment.end_ms) - left };
      }),
    ticks: store.completions
      .filter((tick) => tick.at_ms >= fromMs && tick.at_ms <= toMs)
      .map((tick) => ({ ...tick, left: position(tick.at_ms) })),
  };
}

const HOUR = 3_600_000;

/** Whole local hours inside the window for the axis, thinned to at most `max` labels. */
export function hourTicks(fromMs: number, toMs: number, max = 7): number[] {
  if (toMs <= fromMs) return [];
  const step = Math.max(1, Math.ceil((toMs - fromMs) / HOUR / max)) * HOUR;
  const first = new Date(fromMs);
  first.setMinutes(0, 0, 0);
  const out: number[] = [];
  for (let at = first.getTime() < fromMs ? first.getTime() + HOUR : first.getTime(); at <= toMs; at += step)
    out.push(at);
  return out;
}

/** "2h 05m", "45m", "under a minute". */
export function formatSegmentDuration(ms: number): string {
  const minutes = Math.floor(ms / 60_000);
  if (minutes < 1) return 'under a minute';
  const hours = Math.floor(minutes / 60);
  return hours ? `${String(hours)}h ${String(minutes % 60).padStart(2, '0')}m` : `${String(minutes)}m`;
}

export type OvernightWindow = 'standby' | '12h' | '24h';

/** The window's start: since standby was entered, or the last 12 or 24 hours. */
export function overnightFrom(window: OvernightWindow, nowMs: number, standbyEnteredAtMs: number | undefined): number {
  if (window === 'standby' && standbyEnteredAtMs !== undefined) return Math.min(standbyEnteredAtMs, nowMs - 60_000);
  return nowMs - (window === '24h' ? 24 : 12) * HOUR;
}
