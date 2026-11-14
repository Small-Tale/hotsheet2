import type { CalibrationReport, LiveStoreState } from './api';
import type { HostHealthSignalId, HostHealthSnapshot } from './host-health';
import { type GlanceIcon, HOST_SIGNAL_ICONS, LIVE_STATE_ICONS, RUN_FINISHED_ICON } from './live-state-icons';
import { formatBytes } from './stats-this-mac';

/**
 * The while-you-were-away digest (HS2-JJ9RYN, docs/34-glance-surfaces.md §34.9, mock
 * `m7-away-digest-light`). While standby shows, a recorder watches the pushed live state and host
 * health it already receives and notes what happened; it never issues a request. On exit, the
 * digest adds completions read once from each open project, and shows only when something happened.
 */

/** One thing that happened during standby, with the project or signal it concerns. */
export type AwayEvent =
  | { kind: 'stalled'; store: string; name: string; projectId?: string; slug: string; atMs: number }
  | { kind: 'waiting'; store: string; name: string; projectId?: string; slug: string; title: string; fromMs: number }
  | {
      kind: 'usage_limit';
      store: string;
      name: string;
      worker: string;
      sinceMs: number;
      /** When the tool said the limit resets; `null` when unknown. */
      resetsAtMs: number | null;
      /** When the pause cleared during standby; `undefined` while it still holds. */
      resumedAtMs?: number;
    }
  | { kind: 'host'; signal: HostHealthSignalId; level: 'warning' | 'critical'; atMs: number; detail?: string }
  | { kind: 'run_finished'; store: string; name: string; atMs: number; tickets: number };

export interface AwayRecord {
  enteredAtMs: number;
  /** Each store's snapshot as standby began; a store first seen later counts from its first snapshot. */
  baseline: ReadonlyMap<string, LiveStoreState>;
  events: AwayEvent[];
  /** Token minutes seen during standby, per store and minute start (each snapshot repeats recent minutes). */
  tokenMinutes: Map<string, number>;
  /** Claimed ticket slugs seen per store during its current active run (a finished run's count). */
  worked: Map<string, Set<string>>;
}

export interface AwayDigest {
  enteredAtMs: number;
  exitedAtMs: number;
  completed: number;
  needYou: number;
  /** `null` when neither the server's window read nor the pushed snapshots could count them (HS2-ZTRF4Q). */
  tokens: number | null;
  events: AwayEvent[];
}

const projectFor = (state: LiveStoreState, openProjectIds: readonly string[]) =>
  state.checkouts.find((id) => openProjectIds.includes(id));

/** Start recording at `nowMs` from the stores standby can see. */
export function beginAwayRecord(states: Iterable<LiveStoreState>, nowMs: number): AwayRecord {
  const baseline = new Map<string, LiveStoreState>();
  const worked = new Map<string, Set<string>>();
  for (const state of states) {
    baseline.set(state.store, state);
    worked.set(state.store, new Set(state.claims.map((claim) => claim.slug)));
  }
  return { enteredAtMs: nowMs, baseline, events: [], tokenMinutes: new Map(), worked };
}

/** Note each store's changes since its previous snapshot. `previous` is what standby last saw. */
export function observeAwayStates(
  record: AwayRecord,
  previous: ReadonlyMap<string, LiveStoreState>,
  next: Iterable<LiveStoreState>,
  openProjectIds: readonly string[],
  nowMs: number,
): void {
  for (const state of next) {
    const before = previous.get(state.store) ?? record.baseline.get(state.store);
    for (const minute of state.rates.tokens.minutes) {
      if (minute.start_ms < record.enteredAtMs - 60_000 || minute.start_ms > nowMs) continue;
      const key = `${state.store}\0${minute.start_ms}`;
      record.tokenMinutes.set(key, Math.max(record.tokenMinutes.get(key) ?? 0, minute.tokens));
    }
    const worked = record.worked.get(state.store) ?? new Set<string>();
    record.worked.set(state.store, worked);
    for (const claim of state.claims) worked.add(claim.slug);
    if (!before) continue;
    if (state === before) continue;
    const projectId = projectFor(state, openProjectIds);
    const stalledBefore = new Set(before.activity.stalled.map((entry) => entry.slug));
    for (const entry of state.activity.stalled)
      if (!stalledBefore.has(entry.slug))
        record.events.push({
          kind: 'stalled',
          store: state.store,
          name: state.name,
          ...(projectId ? { projectId } : {}),
          slug: entry.slug,
          atMs: nowMs,
        });
    const waitingBefore = new Set(before.waiting.tickets.map((ticket) => ticket.slug));
    for (const ticket of state.waiting.tickets)
      if (!waitingBefore.has(ticket.slug))
        record.events.push({
          kind: 'waiting',
          store: state.store,
          name: state.name,
          ...(projectId ? { projectId } : {}),
          slug: ticket.slug,
          title: ticket.title,
          fromMs: nowMs,
        });
    const limit = (cause: LiveStoreState['activity']['causes'][number]) => `${cause.worker}\0${cause.since_ms}`;
    const limitsBefore = new Set(before.activity.causes.filter((cause) => cause.kind === 'usage_limit').map(limit));
    const limitsNow = new Set(state.activity.causes.filter((cause) => cause.kind === 'usage_limit').map(limit));
    for (const cause of state.activity.causes)
      if (cause.kind === 'usage_limit' && !limitsBefore.has(limit(cause)))
        record.events.push({
          kind: 'usage_limit',
          store: state.store,
          name: state.name,
          worker: cause.worker,
          sinceMs: cause.since_ms,
          resetsAtMs: cause.resets_at_ms,
        });
    for (const event of record.events)
      if (
        event.kind === 'usage_limit' &&
        event.store === state.store &&
        event.resumedAtMs === undefined &&
        !limitsNow.has(`${event.worker}\0${event.sinceMs}`)
      )
        event.resumedAtMs = nowMs;
    // Active to Stopped means every claim was released. Without queue runway (HS2-5A83MR) the
    // snapshot no longer says whether Up Next still holds ready work, so the row names the run.
    if (before.state === 'active' && state.state === 'stopped') {
      record.events.push({
        kind: 'run_finished',
        store: state.store,
        name: state.name,
        atMs: nowMs,
        tickets: worked.size,
      });
      record.worked.set(state.store, new Set());
    }
  }
}

function hostDetail(snapshot: HostHealthSnapshot, signal: HostHealthSignalId): string | undefined {
  const signals = snapshot.signals;
  switch (signal) {
    case 'disk':
      return signals.disk.free_bytes === null ? undefined : `${formatBytes(signals.disk.free_bytes)} free`;
    case 'power':
      return signals.power.battery_percent === null ? undefined : `${signals.power.battery_percent}% battery`;
    case 'cpu':
      return signals.cpu.busy_percent === null ? undefined : `${Math.round(signals.cpu.busy_percent)}% busy`;
    case 'memory':
    case 'network':
    case 'thermal':
      return undefined;
    default:
      return signal satisfies never;
  }
}

const RANK = { normal: 0, warning: 1, critical: 2, unsupported: 0, unavailable: 0 } as const;

/** Note each host signal that rose to warning or critical since the previous snapshot. */
export function observeAwayHost(
  record: AwayRecord,
  previous: HostHealthSnapshot | undefined,
  next: HostHealthSnapshot,
  nowMs: number,
): void {
  for (const signal of Object.keys(next.signals) as HostHealthSignalId[]) {
    const status = next.signals[signal].status;
    if (status !== 'warning' && status !== 'critical') continue;
    if (previous && RANK[previous.signals[signal].status] >= RANK[status]) continue;
    const detail = hostDetail(next, signal);
    record.events.push({ kind: 'host', signal, level: status, atMs: nowMs, ...(detail ? { detail } : {}) });
  }
}

/** Distinct completions across the open projects' reports inside the standby window. */
export function completionsBetween(reports: readonly CalibrationReport[], fromMs: number, toMs: number): number {
  const slugs = new Set<string>();
  for (const report of reports)
    for (const event of report.events) {
      const at = Date.parse(event.completed_at);
      if (Number.isFinite(at) && at >= fromMs && at <= toMs) slugs.add(`${event.slug}\0${event.completed_at}`);
    }
  return slugs.size;
}

/** The digest to show on exit, or `undefined` when nothing happened (an uneventful standby exits silently). */
export function finishAwayDigest(
  record: AwayRecord,
  current: Iterable<LiveStoreState>,
  completed: number,
  nowMs: number,
  /** Tokens the server recorded across the whole window, read once on exit (HS2-ZTRF4Q); `undefined` when
   * that read failed. Pushed snapshots carry only five minutes, so a longer gap between them loses tokens. */
  windowTokens?: number,
): AwayDigest | undefined {
  let recorded = 0;
  for (const [key, value] of record.tokenMinutes) {
    const start = Number(key.slice(key.indexOf('\0') + 1));
    if (start >= record.enteredAtMs - 60_000 && start <= nowMs) recorded += value;
  }
  // The window read covers every minute the snapshots saw; never report fewer than were seen. With no
  // window read and nothing seen, the count is unknown rather than zero.
  const tokens = windowTokens === undefined ? (recorded > 0 ? recorded : null) : Math.max(windowTokens, recorded);
  let needYou = 0;
  for (const state of current) needYou += state.waiting.count;
  if (!record.events.length && completed === 0 && !tokens) return undefined;
  return {
    enteredAtMs: record.enteredAtMs,
    exitedAtMs: nowMs,
    completed,
    needYou,
    tokens,
    events: [...record.events].sort((a, b) => eventTime(a) - eventTime(b)),
  };
}

export function eventTime(event: AwayEvent): number {
  switch (event.kind) {
    case 'waiting':
      return event.fromMs;
    case 'usage_limit':
      return event.sinceMs;
    case 'stalled':
    case 'host':
    case 'run_finished':
      return event.atMs;
    default:
      return event satisfies never;
  }
}

/** "7h 26m", "26m", "under a minute". */
export function formatAwayDuration(ms: number): string {
  const minutes = Math.floor(ms / 60_000);
  if (minutes < 1) return 'under a minute';
  const hours = Math.floor(minutes / 60);
  return hours ? `${hours}h ${minutes % 60}m` : `${minutes}m`;
}

const TIME = new Intl.DateTimeFormat(undefined, { hour: 'numeric', minute: '2-digit' });
/** "2:35 AM" in the viewer's locale. */
export const awayClock = (ms: number) => TIME.format(new Date(ms));

const HOST_WORDS: Readonly<Record<HostHealthSignalId, string>> = {
  disk: 'Disk space ran low',
  memory: 'Memory pressure rose',
  cpu: 'The CPU stayed busy',
  power: 'Power put the host at risk of sleeping',
  network: 'The network went unreachable',
  thermal: 'The CPU throttled for heat',
};

/** One digest row: its glyph, sentence (a strong subject, then the rest), and its one resolving action. */
export interface AwayEventRow {
  icon: GlanceIcon;
  subject?: string;
  text: string;
  action?: { label: 'Open' | 'Answer'; projectId: string; slug: string };
}

export function awayEventRow(event: AwayEvent, clock: (ms: number) => string = awayClock): AwayEventRow {
  switch (event.kind) {
    case 'stalled':
      return {
        icon: LIVE_STATE_ICONS.stalled,
        subject: event.name,
        text: `stalled at ${clock(event.atMs)} · ${event.slug}`,
        ...(event.projectId
          ? { action: { label: 'Open' as const, projectId: event.projectId, slug: event.slug } }
          : {}),
      };
    case 'waiting':
      return {
        icon: LIVE_STATE_ICONS.waiting_on_you,
        subject: event.name,
        text: `waited on you from ${clock(event.fromMs)} · ${event.slug}`,
        ...(event.projectId
          ? { action: { label: 'Answer' as const, projectId: event.projectId, slug: event.slug } }
          : {}),
      };
    case 'usage_limit':
      return {
        icon: LIVE_STATE_ICONS.halted,
        subject: event.name,
        text:
          event.resumedAtMs !== undefined
            ? `paused ${formatAwayDuration(event.resumedAtMs - event.sinceMs)} at a usage limit (resumed ${clock(event.resumedAtMs)})`
            : event.resetsAtMs !== null
              ? `paused at a usage limit at ${clock(event.sinceMs)} (resumes ${clock(event.resetsAtMs)})`
              : `paused at a usage limit at ${clock(event.sinceMs)}`,
      };
    case 'host':
      return {
        icon: HOST_SIGNAL_ICONS[event.signal],
        text: `${HOST_WORDS[event.signal]}${event.detail ? ` (${event.detail})` : ''} at ${clock(event.atMs)}`,
      };
    case 'run_finished':
      return {
        icon: RUN_FINISHED_ICON,
        subject: event.name,
        text: `finished its run at ${clock(event.atMs)}${
          event.tickets ? ` · ${String(event.tickets)} ${event.tickets === 1 ? 'ticket' : 'tickets'}` : ''
        }`,
      };
    default:
      return event satisfies never;
  }
}
