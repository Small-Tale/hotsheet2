import type { LiveStoreState, LiveWord } from './api';
import type { HostHealthSignalId, HostHealthSnapshot } from './host-health';
import { hostHealthCallouts } from './host-health';
import {
  aggregateLiveStates,
  etaOverrunMs,
  progressAgeMs,
  ticketsUpdatedPerHour,
  tokensPerMinute,
  worstLiveWord,
} from './live-state';

/**
 * The project sidebar's Live section numbers (HS2-F788GW, docs/34-glance-surfaces.md §34.6), derived
 * from the stores a project links and the server host's health. Pure: the caller passes the render
 * clock, so a display timer only re-renders and never reads.
 */

const MINUTE_MS = 60_000;

/** A short duration for titles and the expanded rows: `<1m`, `18m`, `3h 40m`, `2d 4h`. */
export function formatGlanceDuration(ms: number): string {
  const minutes = Math.max(0, Math.floor(ms / MINUTE_MS));
  if (minutes < 1) return '<1m';
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return minutes % 60 ? `${hours}h ${minutes % 60}m` : `${hours}h`;
  const days = Math.floor(hours / 24);
  return hours % 24 ? `${days}d ${hours % 24}h` : `${days}d`;
}

/** The state words a person reads (docs/32-notifier-plugins.md §32.8.1 plus Waiting on you). */
export const LIVE_WORD_LABELS: Readonly<Record<LiveWord, string>> = {
  active: 'Active',
  stopped: 'Stopped',
  may_be_stuck: 'May be stuck',
  stalled: 'Stalled',
  halted: 'Halted',
  waiting_on_you: 'Waiting on you',
};

/** The words that brighten a compact surface (dark cockpit, §34.1); Waiting on you has its own chip. */
export const PROBLEM_LIVE_WORDS: readonly LiveWord[] = ['may_be_stuck', 'stalled', 'halted'];

export interface ProjectLiveOverrun {
  slug: string;
  title: string;
  ms: number;
}

export interface ProjectLiveSummary {
  /** The worst glance word, Waiting on you included (§34.2). */
  state: LiveWord;
  /** The worst work-state word without Waiting on you, which has its own chip and row. */
  activity: LiveWord;
  tokensPerMinute: number;
  tokensPeakPerMinute: number;
  ticketsUpdatedPerHour: number;
  ticketsPeakPerHour: number;
  workersLive: number;
  workersExpected: number;
  /** Time since the most recent progress across the project's stores; absent before any. */
  progressAgeMs?: number;
  waiting: number;
  /** The claim furthest past its ETA; absent while every claim is on time. */
  overrun?: ProjectLiveOverrun;
  /** How many claims are past their ETA. */
  overrunCount: number;
}

/** The project's live numbers, or `undefined` when none of its stores has reported yet. */
export function projectLiveSummary(states: readonly LiveStoreState[], nowMs: number): ProjectLiveSummary | undefined {
  if (!states.length) return undefined;
  const aggregate = aggregateLiveStates(states, nowMs);
  const ages = states.flatMap((state) => {
    const age = progressAgeMs(state, nowMs);
    return age === undefined ? [] : [age];
  });
  const overruns = states
    .flatMap((state) => state.claims)
    .flatMap((claim) => {
      const ms = etaOverrunMs(claim, nowMs);
      return ms === undefined ? [] : [{ slug: claim.slug, title: claim.title, ms }];
    })
    .sort((a, b) => b.ms - a.ms);
  return {
    state: worstLiveWord(states),
    activity: worstLiveWord(states.map((state) => ({ ...state, state: state.activity.word }))),
    tokensPerMinute: states.reduce((sum, state) => sum + tokensPerMinute(state, nowMs), 0),
    tokensPeakPerMinute: aggregate.tokensPeakPerMinute,
    ticketsUpdatedPerHour: states.reduce((sum, state) => sum + ticketsUpdatedPerHour(state, nowMs), 0),
    ticketsPeakPerHour: aggregate.ticketsPeakPerHour,
    workersLive: aggregate.workersLive,
    workersExpected: aggregate.workersExpected,
    ...(ages.length ? { progressAgeMs: Math.min(...ages) } : {}),
    waiting: aggregate.waiting,
    ...(overruns[0] ? { overrun: overruns[0] } : {}),
    overrunCount: overruns.length,
  };
}

/** The work-state sentence for titles and the expanded row, e.g. `Active · 2 workers live`. */
export function liveStateDescription(summary: ProjectLiveSummary): string {
  const workers = `${summary.workersLive} worker${summary.workersLive === 1 ? '' : 's'} live`;
  return `${LIVE_WORD_LABELS[summary.activity]} · ${workers}`;
}

/** The last-progress phrase, e.g. `last progress 2m ago`, or `no progress yet`. */
export function lastProgressDescription(summary: ProjectLiveSummary): string {
  return summary.progressAgeMs === undefined
    ? 'no progress yet'
    : `last progress ${formatGlanceDuration(summary.progressAgeMs)} ago`;
}

/** One compact chip: a glyph and a raw number, with its explanation only in `title` and `label`. */
export interface LiveChip {
  kind: 'state' | 'waiting' | 'overrun';
  /** The raw number or short value on the chip's face; empty for a glyph-only chip. */
  text: string;
  /** The hover title and accessible name. */
  title: string;
  tone: 'neutral' | 'pop' | 'warning' | 'danger';
}

/**
 * The compact chips that have something to say, in reading order (§34.4): a problem state, Needs you,
 * then the worst ETA overrun. A quiet, healthy project shows no chips; queue runway is gone (HS2-5A83MR).
 */
export function liveChips(summary: ProjectLiveSummary): LiveChip[] {
  const chips: LiveChip[] = [];
  if (PROBLEM_LIVE_WORDS.includes(summary.activity))
    chips.push({
      kind: 'state',
      text: '',
      title: `${LIVE_WORD_LABELS[summary.activity]} · ${lastProgressDescription(summary)}`,
      tone: summary.activity === 'may_be_stuck' ? 'warning' : 'danger',
    });
  if (summary.waiting > 0)
    chips.push({
      kind: 'waiting',
      text: String(summary.waiting),
      title: `Needs you · ${summary.waiting} ticket${summary.waiting === 1 ? '' : 's'} waiting on you`,
      tone: 'pop',
    });
  if (summary.overrun)
    chips.push({
      kind: 'overrun',
      text: `+${formatGlanceDuration(summary.overrun.ms)}`,
      title: `${summary.overrun.slug} past ETA by ${formatGlanceDuration(summary.overrun.ms)}${
        summary.overrunCount > 1 ? ` · ${summary.overrunCount} claims past ETA` : ''
      }`,
      tone: 'warning',
    });
  return chips;
}

/** One host condition past its threshold: always a full-width callout, even in compact mode (§34.1). */
export interface HostCallout {
  signal: HostHealthSignalId;
  level: 'warning' | 'critical';
  text: string;
}

const GIB = 1024 ** 3;
const MIB = 1024 ** 2;

function hostCalloutText(snapshot: HostHealthSnapshot, id: HostHealthSignalId): string {
  const { signals } = snapshot;
  switch (id) {
    case 'disk': {
      const { free_bytes: free, hours_to_full: hours } = signals.disk;
      const parts = [free === null ? 'Disk low' : `Disk ${Math.round(free / GIB)} GB free`];
      if (hours !== null) parts.push(`full in ~${formatGlanceDuration(hours * 60 * MINUTE_MS)}`);
      return parts.join(' · ');
    }
    case 'memory': {
      const { reason, swap_growth_bytes_per_hour: growth } = signals.memory;
      if (reason === 'swap_growth' && growth !== null) return `Swap growing ${Math.round(growth / MIB)} MB/h`;
      return `Memory pressure ${signals.memory.pressure ?? signals.memory.status}`;
    }
    case 'cpu': {
      const { busy_percent: busy, top_process: top } = signals.cpu;
      return `CPU ${busy === null ? 'busy' : `${Math.round(busy)}% busy`}${top ? ` · ${top}` : ''}`;
    }
    case 'power': {
      const { reason, battery_percent: battery } = signals.power;
      if (reason === 'lid_closed_on_battery') return 'Lid closed on battery';
      if (reason === 'idle_sleep_allowed') return 'Idle sleep allowed';
      return `On battery${battery === null ? '' : ` · ${Math.round(battery)}%`}`;
    }
    case 'network':
      return 'No network route';
    case 'thermal':
      return 'CPU throttled';
    default:
      return id satisfies never;
  }
}

/** The host conditions to call out, worst first; empty while the machine is normal. */
export function hostCallouts(snapshot: HostHealthSnapshot | undefined): HostCallout[] {
  if (!snapshot) return [];
  return hostHealthCallouts(snapshot).map((signal) => ({
    signal,
    level: snapshot.signals[signal].status === 'critical' ? 'critical' : 'warning',
    text: hostCalloutText(snapshot, signal),
  }));
}

/** The live worker count the Workers view choice names (HS2-6B9S7P); absent at 0 workers. */
export interface WorkersLive {
  live: number;
  progressAgeMs?: number;
}

/** The project's live workers for the Workers view choice, or `undefined` when none are live. */
export function workersLive(summary: ProjectLiveSummary | undefined): WorkersLive | undefined {
  if (!summary || summary.workersLive <= 0) return undefined;
  return {
    live: summary.workersLive,
    ...(summary.progressAgeMs === undefined ? {} : { progressAgeMs: summary.progressAgeMs }),
  };
}

/** The Workers choice's hover title, e.g. `2 workers live · last progress 2m ago`. */
export function workersLiveTitle(workers: WorkersLive): string {
  const count = `${workers.live} worker${workers.live === 1 ? '' : 's'} live`;
  return workers.progressAgeMs === undefined
    ? `${count} · no progress yet`
    : `${count} · last progress ${formatGlanceDuration(workers.progressAgeMs)} ago`;
}
