/**
 * HS2-XJEH08: the orchestration overview's Progress and Projects (design HS2-2XJSJK), from the timeline the
 * view holds and the progress and cost summary (`GET .../orchestration/summary`, docs/26 §26.21). Pure.
 * A measure the summary does not know reads as unknown with its reason, never as zero, and a cost total is
 * labelled partial unless the summary says it is complete.
 */
import type { OrchestrationTimeline, TimelineItem } from './orchestration-timeline';

/** A duration aggregate (`DurationAggregate`). */
export interface DurationTotals {
  known: number;
  unknown: number;
  total_ms: number;
  unknown_reasons: Record<string, number>;
}

/** A cost aggregate (`CostAggregate`). */
export interface CostTotals {
  known_usd: number;
  complete: boolean;
  items_known: number;
  items_unknown: number;
  unpriced_events: number;
  unknown_reasons: Record<string, number>;
}

export interface ProgressFigures {
  lead_time?: DurationTotals;
  cost?: CostTotals;
}

/** What the overview needs of the summary: the totals and each target's figures. */
export interface ProgressSummaryView {
  totals: ProgressFigures;
  targets: Partial<Record<string, ProgressFigures>>;
}

const words = (code: string) => code.replaceAll('_', ' ');
const count = (value: unknown) => (typeof value === 'number' && Number.isFinite(value) ? value : 0);
const reasons = (value: unknown): Record<string, number> => {
  if (!value || typeof value !== 'object') return {};
  return Object.fromEntries(Object.entries(value as Record<string, unknown>).map(([code, n]) => [code, count(n)]));
};

function figures(value: unknown): ProgressFigures {
  const rollup = (value ?? {}) as Record<string, unknown>;
  const lead = rollup.lead_time as Record<string, unknown> | undefined,
    cost = rollup.cost as Record<string, unknown> | undefined;
  return {
    lead_time: lead
      ? {
          known: count(lead.known),
          unknown: count(lead.unknown),
          total_ms: count(lead.total_ms),
          unknown_reasons: reasons(lead.unknown_reasons),
        }
      : undefined,
    cost: cost
      ? {
          known_usd: count(cost.known_usd),
          complete: cost.complete === true,
          items_known: count(cost.items_known),
          items_unknown: count(cost.items_unknown),
          unpriced_events: count(cost.unpriced_events),
          unknown_reasons: reasons(cost.unknown_reasons),
        }
      : undefined,
  };
}

/** Read the summary's totals and per-target figures; a missing part stays absent (unknown, not zero). */
export function parseProgressSummary(value: Record<string, unknown>): ProgressSummaryView {
  const targets = Array.isArray(value.targets) ? (value.targets as Array<Record<string, unknown>>) : [];
  return {
    totals: figures(value.totals),
    targets: Object.fromEntries(
      targets.flatMap((target) =>
        // A target's figures are flattened beside its id (`TargetProgress`).
        typeof target.target_id === 'string' ? [[target.target_id, figures(target)]] : [],
      ),
    ),
  };
}

/** Money as recorded: cents always, and the sub-cent digits a small amount has, never rounded to zero. */
export function moneyText(usd: number): string {
  return new Intl.NumberFormat('en-US', {
    style: 'currency',
    currency: 'USD',
    minimumFractionDigits: 2,
    maximumFractionDigits: 4,
  }).format(usd);
}

/**
 * HS2-RNFEYR: each reason code of `UnknownReason` (crates/hotsheet-ticketing/src/orchestration/summary.rs) in
 * plain words, as it reads inside the parentheses of "Unknown (…)" or "2 work items have no cost (…)". A
 * policy test keeps this in step with the Rust enum; a code it does not know yet reads as words.
 */
export const UNKNOWN_REASONS: Readonly<Record<string, string>> = {
  run_unfinished: 'run not finished yet',
  never_dispatched: 'never claimed in its project',
  history_truncated: 'older run history no longer kept',
  events_trimmed: 'older run events removed',
  clock_inconsistent: 'recorded times run backwards',
  no_gate_linked: 'no CI gate linked',
  no_deployment_requested: 'no deployment requested',
  no_review_requested: 'no review requested',
  gate_unverified: 'no trustworthy gate verdict yet',
  target_unbound: 'project not set up on this machine',
  target_remote: 'project on another machine that does not share its cost',
  usage_source_unsupported: 'tickets not in a Git store, where usage is kept',
  usage_store_unavailable: 'usage records could not be read',
  no_usage_recorded: 'no usage recorded',
  cost_not_read: 'cost could not be read',
  target_unreachable: 'project’s machine did not answer',
  decisions_trimmed: 'older scheduler decisions removed',
  cap_not_configured: 'no cap configured',
};

export const unknownReasonText = (code: string) => UNKNOWN_REASONS[code] ?? words(code);

const why = (codes: Record<string, number>) => {
  const listed = Object.entries(codes)
    .filter(([, n]) => n > 0)
    .map(([code]) => unknownReasonText(code));
  return listed.length > 0 ? listed.join(', ') : 'not reported';
};

function minutes(ms: number): string {
  const total = Math.max(1, Math.round(ms / 60_000));
  if (total < 60) return `${total} min`;
  const hours = Math.floor(total / 60),
    rest = total % 60;
  return rest === 0 ? `${hours} h` : `${hours} h ${rest} min`;
}

/** "About 52 min on average over 3 runs · 1 unknown (run unfinished)", or "Unknown (run unfinished)". */
export function leadTimeText(lead: DurationTotals | undefined): string {
  // HS2-70MHXJ: a summary that leaves the measure out says nothing about it; never "not read".
  if (!lead) return 'Unknown (not reported)';
  if (lead.known === 0) return lead.unknown === 0 ? 'Unknown (no run yet)' : `Unknown (${why(lead.unknown_reasons)})`;
  const mean = lead.total_ms / lead.known,
    runs = `${lead.known} ${lead.known === 1 ? 'run' : 'runs'}`;
  const average =
    mean < 60_000 ? `Under a minute on average over ${runs}` : `About ${minutes(mean)} on average over ${runs}`;
  return lead.unknown > 0 ? `${average} · ${lead.unknown} unknown (${why(lead.unknown_reasons)})` : average;
}

/**
 * "$12.40", "$12.40 so far · partial: 2 work items have no cost (no usage recorded)", "At least $3.00 …"
 * when events have no price, or "Unknown (no usage recorded)" when nothing is known. Never "$0.00" for
 * an unknown cost.
 */
export function costText(cost: CostTotals | undefined): string {
  if (!cost) return 'Unknown (not reported)';
  if (cost.items_known === 0)
    return cost.items_unknown === 0 ? 'Unknown (no work yet)' : `Unknown (${why(cost.unknown_reasons)})`;
  const amount = `${cost.unpriced_events > 0 ? 'At least ' : ''}${moneyText(cost.known_usd)}`;
  if (cost.complete) return amount;
  const missing =
    cost.items_unknown > 0
      ? `${cost.items_unknown} work ${cost.items_unknown === 1 ? 'item has' : 'items have'} no cost (${why(cost.unknown_reasons)})`
      : `${cost.unpriced_events} ${cost.unpriced_events === 1 ? 'event has' : 'events have'} no price`;
  return `${amount} so far · partial: ${missing}`;
}

const DONE = new Set(['done']),
  WORKING = new Set(['running', 'provisional']),
  FOR_YOU = new Set(['waits_for_permission', 'waits_for_decision']);

function tally(items: readonly TimelineItem[]) {
  return {
    total: items.length,
    done: items.filter((item) => DONE.has(item.state)).length,
    working: items.filter((item) => WORKING.has(item.state)).length,
    forYou: items.filter((item) => FOR_YOU.has(item.state)).length,
  };
}

/** HS2-70MHXJ: what a measure reads before the summary is read, or when its read failed. */
const UNREAD = 'Unknown (the summary could not be read)';

/**
 * The Progress rows: Done, Working now, Waiting (with how many wait for a person), Lead time, Cost.
 * HS2-70MHXJ: `unreadable` is a summary read that failed; until one is read the rows say Reading….
 */
export function progressRows(
  timeline: OrchestrationTimeline,
  summary: ProgressSummaryView | undefined,
  unreadable = false,
): Array<{ label: string; value: string }> {
  const pending = unreadable ? UNREAD : 'Reading…';
  const counts = tally(timeline.waves.flatMap((wave) => wave.items)),
    waiting = counts.total - counts.done - counts.working;
  return [
    { label: 'Done', value: `${counts.done} of ${counts.total}` },
    { label: 'Working now', value: String(counts.working) },
    { label: 'Waiting', value: counts.forYou > 0 ? `${waiting} (${counts.forYou} for you)` : String(waiting) },
    { label: 'Lead time', value: summary ? leadTimeText(summary.totals.lead_time) : pending },
    { label: 'Cost', value: summary ? costText(summary.totals.cost) : pending },
  ];
}

/**
 * The Projects rows: per target, its done and working steps and its cost. HS2-70MHXJ: the summary lists only
 * targets with recorded work, so one it leaves out has no work yet (or none recorded on this machine), never
 * "not read"; `unreadable` is a summary read that failed.
 */
export function projectRows(
  timeline: OrchestrationTimeline,
  summary: ProgressSummaryView | undefined,
  unreadable = false,
): Array<{ label: string; value: string }> {
  const items = timeline.waves.flatMap((wave) => wave.items);
  return timeline.targets.map((target) => {
    const counts = tally(items.filter((item) => item.target.target_id === target.target_id)),
      figures = summary?.targets[target.target_id];
    if (summary && !figures) {
      const none = counts.done + counts.working === 0 ? 'no work yet' : 'no cost recorded yet';
      return { label: target.label, value: `${counts.done} done · ${counts.working} working · ${none}` };
    }
    const cost = summary ? costText(figures?.cost) : unreadable ? 'Unknown (could not be read)' : 'reading…';
    const costPart = cost.startsWith('Unknown') ? `cost ${cost.charAt(0).toLowerCase()}${cost.slice(1)}` : cost;
    return { label: target.label, value: `${counts.done} done · ${counts.working} working · ${costPart}` };
  });
}
