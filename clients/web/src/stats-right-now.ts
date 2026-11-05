import type { LiveStoreState, LiveWord } from './api';
import { aggregateLiveStates, etaOverrunMs, progressAgeMs, tokensPerMinute } from './live-state';

/**
 * Pure derivations for the Stats page's **Right now** section (HS2-ERTNJ7,
 * docs/34-glance-surfaces.md §34.8). Every number comes from the pushed live state (HS2-JTTQMY)
 * at a caller-supplied `nowMs`, so a local display clock re-renders without a request.
 */

const MINUTE_MS = 60_000;
const HOUR_MS = 60 * MINUTE_MS;

/** Most needy first: the order the Projects table sorts by (§34.8 "sorted by need"). */
export const LIVE_NEED_ORDER: readonly LiveWord[] = [
  'halted',
  'stalled',
  'may_be_stuck',
  'waiting_on_you',
  'active',
  'stopped',
];

/** The visible state word (§34.2 vocabulary). */
export const LIVE_WORD_LABELS: Readonly<Record<LiveWord, string>> = {
  active: 'Active',
  stopped: 'Stopped',
  may_be_stuck: 'May be stuck',
  stalled: 'Stalled',
  halted: 'Halted',
  waiting_on_you: 'Waiting on you',
};

/** Tickets with a progress write in each of `buckets` equal slices of the last hour, oldest first. */
export function updatedSparkline(states: readonly LiveStoreState[], nowMs: number, buckets = 12): number[] {
  const counts = new Array<number>(buckets).fill(0);
  const span = HOUR_MS / buckets;
  const since = nowMs - HOUR_MS;
  for (const state of states)
    for (const at of state.rates.tickets_updated.at_ms) {
      if (at <= since || at > nowMs) continue;
      const index = Math.min(buckets - 1, Math.floor((at - since) / span));
      counts[index] = (counts[index] ?? 0) + 1;
    }
  return counts;
}

export interface RightNowTiles {
  tokensPerMinute: number;
  tokensPeakPerMinute: number;
  ticketsUpdated: number;
  updatedSparkline: number[];
  needsYou: number;
  workersLive: number;
  workersExpected: number;
}

export function rightNowTiles(states: readonly LiveStoreState[], nowMs: number): RightNowTiles {
  const aggregate = aggregateLiveStates(states, nowMs);
  return {
    tokensPerMinute: aggregate.tokensPerMinute,
    tokensPeakPerMinute: aggregate.tokensPeakPerMinute,
    ticketsUpdated: aggregate.ticketsUpdatedPerHour,
    updatedSparkline: updatedSparkline(states, nowMs),
    needsYou: aggregate.waiting,
    workersLive: aggregate.workersLive,
    workersExpected: aggregate.workersExpected,
  };
}

/** One live claim as the Current work column shows it. */
export interface RightNowWork {
  slug: string;
  title: string;
  phase?: string;
}

/** The one action that unsticks a project: answer its waiting ticket, or open its terminal. */
export type RightNowAction =
  { kind: 'answer'; slug: string; projectId: string } | { kind: 'open-terminal'; terminalKey: string };

export interface RightNowRow {
  store: string;
  name: string;
  state: LiveWord;
  work: RightNowWork[];
  /** Feedback and review requests when nothing is claimed. */
  waiting: number;
  progressAgeMs?: number;
  /** The largest ETA overrun among the live claims. */
  etaOverrunMs?: number;
  runwayReady: number;
  runwayEstimateMs?: number;
  tokensPerMinute: number;
  action?: RightNowAction;
}

/** Which client project and terminal a store's actions open; supplied by the app. */
export interface RightNowTargets {
  /** The open client project (checkout id) for a store, preferring the one Stats is scoped to. */
  projectFor: (state: LiveStoreState) => string | undefined;
  /** The terminal to open for a project: a halted one first, else its first terminal. */
  terminalFor: (projectId: string) => string | undefined;
}

const STUCK: ReadonlySet<LiveWord> = new Set(['may_be_stuck', 'stalled', 'halted']);

function rowAction(state: LiveStoreState, targets: RightNowTargets): RightNowAction | undefined {
  const projectId = targets.projectFor(state);
  if (!projectId) return undefined;
  if (state.state === 'waiting_on_you') {
    const ticket = state.waiting.tickets.at(0);
    return ticket ? { kind: 'answer', slug: ticket.slug, projectId } : undefined;
  }
  if (!STUCK.has(state.state)) return undefined;
  const terminalKey = targets.terminalFor(projectId);
  return terminalKey ? { kind: 'open-terminal', terminalKey } : undefined;
}

export function rightNowRow(state: LiveStoreState, nowMs: number, targets: RightNowTargets): RightNowRow {
  const overruns = state.claims
    .map((claim) => etaOverrunMs(claim, nowMs))
    .filter((value): value is number => value !== undefined);
  return {
    store: state.store,
    name: state.name,
    state: state.state,
    work: state.claims.map((claim) => ({
      slug: claim.slug,
      title: claim.title,
      ...(claim.phase ? { phase: claim.phase } : {}),
    })),
    waiting: state.waiting.count,
    progressAgeMs: progressAgeMs(state, nowMs),
    etaOverrunMs: overruns.length ? Math.max(...overruns) : undefined,
    runwayReady: state.runway.ready,
    runwayEstimateMs: state.runway.estimate_ms ?? undefined,
    tokensPerMinute: tokensPerMinute(state, nowMs),
    action: rowAction(state, targets),
  };
}

/** Sorted by need: problem words first, then the longest without progress, then by name. */
export function rightNowRows(
  states: readonly LiveStoreState[],
  nowMs: number,
  targets: RightNowTargets,
): RightNowRow[] {
  return states
    .map((state) => rightNowRow(state, nowMs, targets))
    .sort(
      (a, b) =>
        LIVE_NEED_ORDER.indexOf(a.state) - LIVE_NEED_ORDER.indexOf(b.state) ||
        (b.progressAgeMs ?? -1) - (a.progressAgeMs ?? -1) ||
        a.name.localeCompare(b.name),
    );
}

/** A compact duration: `45s`, `34m`, `1h 05m`, `3d`. */
export function formatDuration(ms: number): string {
  const seconds = Math.max(0, Math.round(ms / 1000));
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  if (hours < 48) return `${hours}h ${String(minutes % 60).padStart(2, '0')}m`;
  return `${Math.floor(hours / 24)}d`;
}

/** Today's completions from the analytics day buckets, which are UTC calendar days. */
export function completedSinceUtcMidnight(
  throughputByDay: Readonly<Record<string, number>>,
  nowMs: number,
): { count: number; sinceMs: number } {
  const day = new Date(nowMs).toISOString().slice(0, 10);
  return { count: throughputByDay[day] ?? 0, sinceMs: Date.parse(`${day}T00:00:00Z`) };
}
