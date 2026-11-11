import type { LiveStoreState, LiveWord } from './api';
import { isRecord } from './json-value';
import { progressAgeMs, ticketsUpdatedPerHour, tokensPerMinute } from './live-state';
import { type ResponseParser, ServerResponseShapeError } from './server-response-parsers';

/**
 * Standby (HS2-EQVZTZ, docs/34-glance-surfaces.md §34.5): the pure parts of the night display. The
 * settings contract, the all-projects display model derived from the pushed live state, and the
 * idle timer live here; `features/standby.tsx` wires them to the page.
 */

/** Effective machine-wide standby settings (`GET /standby-settings`). */
export interface StandbySettingsValues {
  enabled: boolean;
  idle_minutes: number;
}
/** Defaults, explicit values (`null` = default), and effective values. */
export interface StandbySettingsView {
  defaults: StandbySettingsValues;
  configured: { enabled: boolean | null; idle_minutes: number | null };
  effective: StandbySettingsValues;
}

/** The idle thresholds the settings page offers, in minutes. */
export const STANDBY_IDLE_CHOICES = [5, 10, 15, 30, 60, 120, 240] as const;
/** The client-side fallback while the server's answer is unknown; it matches the server default. */
export const STANDBY_DEFAULT_SETTINGS: StandbySettingsValues = { enabled: true, idle_minutes: 15 };

const isOptional = (value: unknown, check: (field: unknown) => boolean) =>
  value === null || value === undefined || check(value);
const isMinutes = (value: unknown) => typeof value === 'number' && Number.isInteger(value) && value > 0;

/** `GET`/`PUT /standby-settings`. */
export const parseStandbySettings: ResponseParser<StandbySettingsView> = (value) => {
  const what = 'standby settings';
  if (!isRecord(value)) throw new ServerResponseShapeError(what);
  for (const group of ['defaults', 'effective', 'configured'] as const) {
    const values = value[group];
    if (!isRecord(values)) throw new ServerResponseShapeError(what);
    const configured = group === 'configured';
    const enabledOk = configured
      ? isOptional(values.enabled, (field) => typeof field === 'boolean')
      : typeof values.enabled === 'boolean';
    const idleOk = configured ? isOptional(values.idle_minutes, isMinutes) : isMinutes(values.idle_minutes);
    if (!enabledOk || !idleOk) throw new ServerResponseShapeError(what);
  }
  return value as unknown as StandbySettingsView;
};

/**
 * What standby shows: always every hosted store, whichever project or page it was entered from
 * (HS2-JGW031). Standby is a whole-machine night display, not a view of the active tab.
 */
export type StandbyScope = { kind: 'all' };

/** The states that get brightness on standby: problems only (dark cockpit, §34.1). */
export const STANDBY_ALERT_WORDS: ReadonlySet<LiveWord> = new Set(['stalled', 'halted', 'waiting_on_you']);

const WORD_LABELS: Readonly<Record<LiveWord, string>> = {
  active: 'Active',
  stopped: 'Stopped',
  may_be_stuck: 'May be stuck',
  stalled: 'Stalled',
  halted: 'Halted',
  waiting_on_you: 'Waiting on you',
};

/** The display word of a live state, as every glance surface names it (§34.2). */
export const standbyWordLabel = (word: LiveWord): string => WORD_LABELS[word];

/** A compact age: `now`, `4m`, `2h 5m`, `3d`. */
export function formatStandbyAge(ms: number): string {
  const minutes = Math.floor(ms / 60_000);
  if (minutes < 1) return 'now';
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return minutes % 60 ? `${hours}h ${minutes % 60}m` : `${hours}h`;
  return `${Math.floor(hours / 24)}d`;
}

/** One row of the per-project strip. */
export interface StandbyRow {
  key: string;
  word: LiveWord;
  title: string;
  detail: string;
  /** A problem state: the row lights up amber. */
  alert: boolean;
}

/** One amber callout. */
export interface StandbyAlert {
  key: string;
  word: LiveWord;
  title: string;
  detail: string;
}

/** Everything the standby display renders, derived locally from the pushed live state. */
export interface StandbyModel {
  scopeLabel: string;
  tokensPerMinute: number;
  tokensPeakPerMinute: number;
  ticketsUpdated: number;
  rows: StandbyRow[];
  alerts: StandbyAlert[];
}

const plural = (count: number, one: string, many = `${one}s`) => `${count} ${count === 1 ? one : many}`;

function stateDetail(state: LiveStoreState, nowMs: number): string {
  const age = progressAgeMs(state, nowMs);
  const ageText = age === undefined ? undefined : formatStandbyAge(age);
  switch (state.state) {
    case 'stalled': {
      const slugs = state.activity.stalled.map((item) => item.slug);
      return [slugs.join(', ') || undefined, 'lease expired', ageText && `no progress for ${ageText}`]
        .filter(Boolean)
        .join(' · ');
    }
    case 'halted':
      return state.activity.halted[0]?.message ?? 'a worker halted';
    case 'waiting_on_you':
      // Only Started tickets with an open feedback request count (HS2-JGW031).
      return plural(state.waiting.count, 'feedback request');
    case 'stopped': {
      // How long ago work stopped; queue runway is gone (HS2-5A83MR).
      const stopped = state.activity.stopped_at_ms;
      return stopped === null ? '' : `for ${formatStandbyAge(Math.max(0, nowMs - stopped))}`;
    }
    case 'active':
    case 'may_be_stuck':
      return [ageText, state.claims[0]?.slug].filter(Boolean).join(' · ');
    default:
      return state.state satisfies never;
  }
}

function alertTitle(state: LiveStoreState): string {
  if (state.state === 'stalled') return `${state.name} stalled`;
  if (state.state === 'halted') return `${state.name} halted`;
  return `${state.name} is waiting on you`;
}

/** One row per store, named by its project (m4/m5). */
function allRows(states: readonly LiveStoreState[], nowMs: number): StandbyRow[] {
  return states.map((state) => ({
    key: state.store,
    word: state.state,
    title: state.name,
    detail: [standbyWordLabel(state.state), stateDetail(state, nowMs)].filter(Boolean).join(' · '),
    alert: STANDBY_ALERT_WORDS.has(state.state),
  }));
}

/** Every store, in name order so a row never jumps when its state changes. */
export function standbyStores(states: Iterable<LiveStoreState>): LiveStoreState[] {
  return [...states].sort((a, b) => a.name.localeCompare(b.name));
}

/** Derive the standby display from the live state model at `nowMs`; it issues no requests. */
export function standbyModel(states: Iterable<LiveStoreState>, nowMs: number): StandbyModel {
  const list = standbyStores(states);
  return {
    scopeLabel: 'All projects',
    tokensPerMinute: list.reduce((sum, state) => sum + tokensPerMinute(state, nowMs), 0),
    tokensPeakPerMinute: list.reduce((sum, state) => sum + state.rates.tokens.peak_per_minute, 0),
    ticketsUpdated: list.reduce((sum, state) => sum + ticketsUpdatedPerHour(state, nowMs), 0),
    rows: allRows(list, nowMs),
    alerts: list
      .filter((state) => STANDBY_ALERT_WORDS.has(state.state))
      .map((state) => ({
        key: state.store,
        word: state.state,
        title: alertTitle(state),
        detail: stateDetail(state, nowMs),
      })),
  };
}

/** The clock face: hours and minutes without a day period, like a bedside clock. */
export function formatStandbyClock(nowMs: number, locale?: string): string {
  return new Intl.DateTimeFormat(locale, { hour: 'numeric', minute: '2-digit' })
    .formatToParts(new Date(nowMs))
    .filter((part) => part.type !== 'dayPeriod')
    .map((part) => part.value)
    .join('')
    .trim();
}

/** Seconds-resolution time for the freshness line. */
export function formatStandbyTime(nowMs: number, locale?: string): string {
  return new Intl.DateTimeFormat(locale, { hour: 'numeric', minute: '2-digit', second: '2-digit' })
    .formatToParts(new Date(nowMs))
    .filter((part) => part.type !== 'dayPeriod')
    .map((part) => part.value)
    .join('')
    .trim();
}

/** The freshness line's state: live, or the change stream dropped so the numbers may be stale. */
export interface StandbyFreshness {
  live: boolean;
  /** When a live snapshot last arrived, if ever. */
  updatedAtMs?: number;
}

/** The freshness line: a frozen screen must never look healthy. */
export function standbyFreshnessLabel(freshness: StandbyFreshness, locale?: string): string {
  const when = freshness.updatedAtMs === undefined ? undefined : formatStandbyTime(freshness.updatedAtMs, locale);
  if (freshness.live) return when ? `live · updated ${when}` : 'live';
  return when ? `connection lost · last update ${when}` : 'connection lost';
}

export interface StandbyIdleTimerOptions {
  /** The idle threshold in milliseconds, or `undefined` while idle entry is off. */
  idleMs: () => number | undefined;
  /** Called once when the threshold passes with no activity. */
  onIdle: () => void;
  now?: () => number;
  setTimer?: (callback: () => void, ms: number) => unknown;
  clearTimer?: (handle: unknown) => void;
}

/**
 * Enters standby after no direct interaction for the idle threshold. Activity only records a time,
 * so a stream of pointer moves costs no timer churn: the one pending timer re-arms itself for the
 * remainder when it fires early. It is a local display timer and never issues a request.
 */
export function createStandbyIdleTimer(options: StandbyIdleTimerOptions) {
  const now = options.now ?? Date.now;
  const setTimer = options.setTimer ?? ((callback: () => void, ms: number) => globalThis.setTimeout(callback, ms));
  const clearTimer =
    options.clearTimer ??
    ((handle: unknown) => {
      globalThis.clearTimeout(handle as ReturnType<typeof setTimeout>);
    });
  let last = now();
  let handle: unknown;
  let paused = false;

  function cancel() {
    if (handle !== undefined) clearTimer(handle);
    handle = undefined;
  }

  function arm() {
    cancel();
    const idle = options.idleMs();
    if (paused || idle === undefined) return;
    handle = setTimer(fire, Math.max(0, last + idle - now()));
  }

  function fire() {
    handle = undefined;
    const idle = options.idleMs();
    if (paused || idle === undefined) return;
    const remaining = last + idle - now();
    if (remaining > 0) {
      handle = setTimer(fire, remaining);
      return;
    }
    paused = true;
    options.onIdle();
  }

  return {
    /** A direct human interaction: restart the idle window. */
    activity() {
      last = now();
      if (handle === undefined && !paused) arm();
    },
    /** The settings changed: re-arm from the last activity with the new threshold. */
    reconfigure: arm,
    /** Stop counting (standby is showing). */
    pause() {
      paused = true;
      cancel();
    },
    /** Count again from now (standby exited). */
    resume() {
      paused = false;
      last = now();
      arm();
    },
    dispose() {
      paused = true;
      cancel();
    },
  };
}

export type StandbyIdleTimer = ReturnType<typeof createStandbyIdleTimer>;
