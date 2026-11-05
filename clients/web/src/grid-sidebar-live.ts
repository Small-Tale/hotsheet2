import { type CssForegroundColor, uiColor } from '@kerfjs/ui/css-values';

import type { LiveStoreState, LiveWord } from './api';
import { aggregateLiveStates, progressAgeMs } from './live-state';

/**
 * What the workspace grid sidebar's gauge and triage pages show for one group (HS2-E1AQE8,
 * docs/34-glance-surfaces.md §34.7): a project's linked stores, or every open project's stores for
 * All projects. Derived from the pushed live state (HS2-JTTQMY) at a local render time; nothing here
 * reads the network.
 */
export interface GridSidebarLive {
  state: LiveWord;
  tokensPerMinute: number;
  tokensPeakPerMinute: number;
  ticketsUpdatedPerHour: number;
  ticketsPeakPerHour: number;
  waiting: number;
  workersLive: number;
  workersExpected: number;
  /** Milliseconds since the most recent progress in any of the stores; absent before any. */
  progressAgeMs?: number;
  /** Milliseconds since the stores stopped; set only for Stopped. */
  stoppedAgeMs?: number;
  /** The current work: the most recently claimed live claim. */
  claim?: { slug: string; phase: string | null };
  /** The first ticket waiting on the user. */
  waitingSlug?: string;
  /** The first stalled claim's ticket, for Stalled. */
  stalledSlug?: string;
  /** The first halt message, for Halted. */
  haltedMessage?: string;
}

/** The server's state words as people read them (docs/32-notifier-plugins.md §32.8.1). */
export const LIVE_WORD_LABELS: Readonly<Record<LiveWord, string>> = {
  active: 'Active',
  waiting_on_you: 'Waiting on you',
  may_be_stuck: 'May be stuck',
  stalled: 'Stalled',
  halted: 'Halted',
  stopped: 'Stopped',
};

/** Need order for the triage page: problems first, stopped last. */
const NEED: Readonly<Record<LiveWord, number>> = {
  halted: 5,
  stalled: 4,
  may_be_stuck: 3,
  waiting_on_you: 2,
  active: 1,
  stopped: 0,
};

/** How much a state needs the user; higher sorts first on the triage page. */
export function liveNeed(word: LiveWord | undefined): number {
  return word === undefined ? -1 : NEED[word];
}

/**
 * The glyph colour for a state (dark cockpit, §34.1): Stopped stays quiet and only problems take a
 * warning or danger tone. Active keeps the quiet success tone of the approved mocks.
 */
export function liveWordColor(word: LiveWord): CssForegroundColor {
  switch (word) {
    case 'active':
      return uiColor('success-on-quiet');
    case 'waiting_on_you':
      return uiColor('pop-on-quiet');
    case 'may_be_stuck':
      return uiColor('warning-on-quiet');
    case 'stalled':
    case 'halted':
      return uiColor('danger-on-quiet');
    case 'stopped':
      return uiColor('text-quiet');
  }
}

/** Combine one group's stores at `nowMs`, or `undefined` when no store reports live state. */
export function gridSidebarLive(states: readonly LiveStoreState[], nowMs: number): GridSidebarLive | undefined {
  if (states.length === 0) return undefined;
  const aggregate = aggregateLiveStates(states, nowMs);
  const ages = states.map((state) => progressAgeMs(state, nowMs)).filter((age): age is number => age !== undefined);
  const stoppedAt = states
    .map((state) => state.activity.stopped_at_ms)
    .filter((at): at is number => at !== null)
    .reduce<number | undefined>((latest, at) => (latest === undefined || at > latest ? at : latest), undefined);
  const claim = states
    .flatMap((state) => state.claims)
    .reduce<LiveStoreState['claims'][number] | undefined>(
      (latest, next) =>
        latest === undefined || (next.claimed_at_ms ?? 0) > (latest.claimed_at_ms ?? 0) ? next : latest,
      undefined,
    );
  const waitingSlug = states.flatMap((state) => state.waiting.tickets)[0]?.slug;
  const stalledSlug = states.flatMap((state) => state.activity.stalled)[0]?.slug;
  const haltedMessage = states.flatMap((state) => state.activity.halted)[0]?.message;
  return {
    state: aggregate.state,
    tokensPerMinute: aggregate.tokensPerMinute,
    tokensPeakPerMinute: aggregate.tokensPeakPerMinute,
    ticketsUpdatedPerHour: aggregate.ticketsUpdatedPerHour,
    ticketsPeakPerHour: aggregate.ticketsPeakPerHour,
    waiting: aggregate.waiting,
    workersLive: aggregate.workersLive,
    workersExpected: aggregate.workersExpected,
    ...(ages.length ? { progressAgeMs: Math.min(...ages) } : {}),
    ...(aggregate.state === 'stopped' && stoppedAt !== undefined
      ? { stoppedAgeMs: Math.max(0, nowMs - stoppedAt) }
      : {}),
    ...(claim ? { claim: { slug: claim.slug, phase: claim.phase } } : {}),
    ...(waitingSlug ? { waitingSlug } : {}),
    ...(stalledSlug ? { stalledSlug } : {}),
    ...(haltedMessage ? { haltedMessage } : {}),
  };
}

/** A duration for a hover title: `less than a minute`, `1 minute`, `34 minutes`, `2 hours`, `3 days`. */
export function formatLiveDuration(ms: number): string {
  const minutes = Math.floor(ms / 60_000);
  if (minutes < 1) return 'less than a minute';
  if (minutes < 60) return `${minutes} minute${minutes === 1 ? '' : 's'}`;
  const hours = Math.floor(minutes / 60);
  if (hours < 48) return `${hours} hour${hours === 1 ? '' : 's'}`;
  const days = Math.floor(hours / 24);
  return `${days} days`;
}

/** The current work on the face of a triage row: `HS2-R76MMW · working`, or the waiting ticket. */
export function liveWorkSummary(live: GridSidebarLive): string | undefined {
  if (live.state === 'waiting_on_you' && live.waitingSlug) return live.waitingSlug;
  if (live.state === 'stalled' && live.stalledSlug) return live.stalledSlug;
  if (!live.claim) return undefined;
  return live.claim.phase ? `${live.claim.slug} · ${live.claim.phase.replace(/_/g, ' ')}` : live.claim.slug;
}

/**
 * The hover title and accessible name for a group's state (compact-surface rule, §34.4): the state
 * word, then the duration or cause that the face leaves out, then worker and waiting counts.
 */
export function liveStateTitle(name: string, live: GridSidebarLive): string {
  const parts = [LIVE_WORD_LABELS[live.state]];
  switch (live.state) {
    case 'active':
      if (live.progressAgeMs !== undefined) parts.push(`last progress ${formatLiveDuration(live.progressAgeMs)} ago`);
      break;
    case 'may_be_stuck':
      if (live.progressAgeMs !== undefined) parts.push(`no progress for ${formatLiveDuration(live.progressAgeMs)}`);
      break;
    case 'stalled':
      if (live.stalledSlug) parts.push(`the lease on ${live.stalledSlug} expired`);
      break;
    case 'halted':
      if (live.haltedMessage) parts.push(live.haltedMessage);
      break;
    case 'waiting_on_you':
      break;
    case 'stopped':
      if (live.stoppedAgeMs !== undefined) parts.push(`for ${formatLiveDuration(live.stoppedAgeMs)}`);
      break;
  }
  if (live.waiting > 0) parts.push(`${live.waiting} waiting on you`);
  if (live.workersExpected > 0 || live.workersLive > 0)
    parts.push(`${live.workersLive} of ${live.workersExpected} workers live`);
  return `${name}: ${parts.join(', ')}`;
}

/** The triage page's order: most need first; equal need keeps the given project order. */
export function triageOrder<T extends { live?: GridSidebarLive }>(groups: readonly T[]): T[] {
  return groups
    .map((group, index) => ({ group, index }))
    .sort((a, b) => liveNeed(b.group.live?.state) - liveNeed(a.group.live?.state) || a.index - b.index)
    .map(({ group }) => group);
}
