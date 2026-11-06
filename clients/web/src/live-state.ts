import { signal } from 'kerfjs';

import type { ChangeEvent, LiveCause, LiveClaim, LiveStateList, LiveStoreState, LiveWord } from './api';
import { parseLiveStoreState } from './server-response-parsers';

/**
 * Client store for the server-pushed per-store live state (HS2-JTTQMY,
 * docs/32-notifier-plugins.md §32.8.3). Glance surfaces (standby, the project and grid
 * sidebars, the stats Right now section, the away digest) bind to `states` and derive their
 * numbers with the pure helpers below.
 *
 * Snapshots arrive as `live_state` change events on each project's existing change stream and
 * are read with `GET /live-state` only when that stream (re)establishes continuity. Nothing
 * here reads on a timer: ages and decaying rates are computed from the snapshot's timestamps,
 * so a display timer only re-renders.
 */

/** Problems first; used to pick the word an aggregate shows. */
const SEVERITY: Record<LiveWord, number> = {
  halted: 5,
  stalled: 4,
  may_be_stuck: 3,
  waiting_on_you: 2,
  active: 1,
  stopped: 0,
};

const MINUTE_MS = 60_000;

/** Whether `next` is newer than `current`: a later server start, or a later revision of it. */
export function isNewerLiveState(current: LiveStoreState | undefined, next: LiveStoreState): boolean {
  if (!current) return true;
  if (next.epoch_ms !== current.epoch_ms) return next.epoch_ms > current.epoch_ms;
  return next.revision > current.revision;
}

/** Tokens per minute at `nowMs`: the window's whole minutes, the current one included. */
export function tokensPerMinute(state: LiveStoreState, nowMs: number): number {
  const { window_minutes: window, minutes } = state.rates.tokens;
  if (window <= 0) return 0;
  const first = Math.floor(nowMs / MINUTE_MS) * MINUTE_MS - (window - 1) * MINUTE_MS;
  const total = minutes
    .filter((minute) => minute.start_ms >= first && minute.start_ms <= nowMs)
    .reduce((sum, minute) => sum + minute.tokens, 0);
  return total / window;
}

/** Distinct tickets with a progress write in the last hour at `nowMs` (claim renewals excluded). */
export function ticketsUpdatedPerHour(state: LiveStoreState, nowMs: number): number {
  const since = nowMs - state.rates.tickets_updated.window_minutes * MINUTE_MS;
  return state.rates.tickets_updated.at_ms.filter((at) => at > since && at <= nowMs).length;
}

/** Milliseconds since the idle detector last saw progress, or `undefined` before any. */
export function progressAgeMs(state: LiveStoreState, nowMs: number): number | undefined {
  const at = state.activity.last_progress_ms;
  return at === null ? undefined : Math.max(0, nowMs - at);
}

/** How far a live claim is past its ETA, or `undefined` while it is on time or has none. */
export function etaOverrunMs(claim: LiveClaim, nowMs: number): number | undefined {
  if (claim.eta_at_ms === null || nowMs <= claim.eta_at_ms) return undefined;
  return nowMs - claim.eta_at_ms;
}

/** A gauge position in [0, 1] against its 30-day peak; any value pins a zero peak's gauge. */
export function gaugeFraction(value: number, peak: number): number {
  if (peak <= 0) return value > 0 ? 1 : 0;
  return Math.min(1, Math.max(0, value / peak));
}

/**
 * When a usage-limit pause ends (HS2-EGKKA0): `undefined` without a pause, `null` while any
 * paused session's tool did not state its reset (unknown), else the latest stated reset, when
 * every paused session can continue. A past time means the limit reset but the session still
 * waits for a new prompt.
 */
export function pausedUntilMs(state: LiveStoreState): number | null | undefined {
  const pauses = state.activity.causes.filter((cause) => cause.kind === 'usage_limit');
  if (pauses.length === 0) return undefined;
  let latest = -Infinity;
  for (const pause of pauses) {
    if (pause.resets_at_ms === null) return null;
    latest = Math.max(latest, pause.resets_at_ms);
  }
  return latest;
}

/** The terminal prompts this store's workers wait at, oldest first (HS2-EGKKA0). */
export function awaitingInputCauses(state: LiveStoreState): LiveCause[] {
  return state.activity.causes.filter((cause) => cause.kind === 'awaiting_input');
}

/** The most severe word among `states`, or `stopped` when there are none. */
export function worstLiveWord(states: Iterable<LiveStoreState>): LiveWord {
  let worst: LiveWord = 'stopped';
  for (const state of states) if (SEVERITY[state.state] > SEVERITY[worst]) worst = state.state;
  return worst;
}

/** All-projects numbers for the aggregate gauges and counts. */
export interface LiveAggregate {
  stores: number;
  state: LiveWord;
  tokensPerMinute: number;
  /** The sum of the stores' peaks: an upper bound, so an aggregate needle never pins early. */
  tokensPeakPerMinute: number;
  ticketsUpdatedPerHour: number;
  ticketsPeakPerHour: number;
  waiting: number;
  workersLive: number;
  workersExpected: number;
}

export function aggregateLiveStates(states: Iterable<LiveStoreState>, nowMs: number): LiveAggregate {
  const list = [...states];
  return {
    stores: list.length,
    state: worstLiveWord(list),
    tokensPerMinute: list.reduce((sum, state) => sum + tokensPerMinute(state, nowMs), 0),
    tokensPeakPerMinute: list.reduce((sum, state) => sum + state.rates.tokens.peak_per_minute, 0),
    ticketsUpdatedPerHour: list.reduce((sum, state) => sum + ticketsUpdatedPerHour(state, nowMs), 0),
    ticketsPeakPerHour: list.reduce((sum, state) => sum + state.rates.tickets_updated.peak_per_hour, 0),
    waiting: list.reduce((sum, state) => sum + state.waiting.count, 0),
    workersLive: list.reduce((sum, state) => sum + state.workers.live, 0),
    workersExpected: list.reduce((sum, state) => sum + state.workers.expected, 0),
  };
}

export interface LiveStateTransport {
  liveStates: () => Promise<LiveStateList>;
}

/** Per-store live snapshots keyed by store id, with ordered apply and resync. */
export function createLiveStateStore() {
  const states = signal<ReadonlyMap<string, LiveStoreState>>(new Map());

  /** Keep `next` when it is newer than what the store holds. Returns whether it changed. */
  function apply(next: LiveStoreState): boolean {
    const current = states.peek().get(next.store);
    if (!isNewerLiveState(current, next)) return false;
    const map = new Map(states.peek());
    map.set(next.store, next);
    states.value = map;
    return true;
  }

  /** Apply the valid `live_state` events of a stream batch. Returns how many changed a store. */
  function applyEvents(events: readonly ChangeEvent[]): number {
    let changed = 0;
    for (const event of events)
      if (event.kind === 'live_state' && parseLiveStoreState.check(event.live) && apply(event.live!)) changed += 1;
    return changed;
  }

  /**
   * Re-read every hosted store after a stream (re)connects or overflows. A store the same
   * server no longer hosts is dropped; snapshots from another server start are kept. A failed
   * read keeps what the store holds and resolves `false`.
   */
  async function resync(transport: LiveStateTransport): Promise<boolean> {
    let list: LiveStateList;
    try {
      list = await transport.liveStates();
    } catch {
      return false;
    }
    for (const state of list.stores) apply(state);
    const hosted = new Set(list.stores.map((state) => state.store));
    const stale = [...states.peek().values()].filter(
      (state) => state.epoch_ms === list.epoch_ms && !hosted.has(state.store),
    );
    if (stale.length) remove(stale.map((state) => state.store));
    return true;
  }

  function remove(stores: readonly string[]) {
    const map = new Map(states.peek());
    let changed = false;
    for (const store of stores) changed = map.delete(store) || changed;
    if (changed) states.value = map;
  }

  /** The stores a client project (a registered checkout id) links. */
  function forProject(projectId: string): LiveStoreState[] {
    return [...states.value.values()].filter((state) => state.checkouts.includes(projectId));
  }

  /**
   * The stores linked to any of these projects, each once. All-projects glance views pass the open
   * project tabs, so a store the server still hosts for a closed project never shows (HS2-JGW031).
   */
  function forProjects(projectIds: Iterable<string>): LiveStoreState[] {
    const open = new Set(projectIds);
    return [...states.value.values()].filter((state) => state.checkouts.some((id) => open.has(id)));
  }

  function reset() {
    states.value = new Map();
  }

  return { states, apply, applyEvents, resync, remove, forProject, forProjects, reset };
}

export type LiveStateStore = ReturnType<typeof createLiveStateStore>;
