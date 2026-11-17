/**
 * HS2-77SRHC: the throughput scheduler (docs/26 §26.17) in words for the Orchestration view: why a ready
 * card is not running (`scheduler explain`), the overview's Scheduler and Latest Decisions sections, and
 * the Show All Decisions… dialog (`scheduler decisions` with `newest`). Pure; codes it does not know read
 * as their words. Identifiers and codes only, as the decision log holds.
 */
import type { OrchestrationTimeline } from './orchestration-timeline';
import { type ClockFormat, toolLabel } from './orchestration-timeline-board';

/** One decision log record (`DecisionRecord`), as much of it as the view shows. */
export interface SchedulerDecision {
  seq: number;
  at_ms: number;
  tick_id: string;
  mode: string;
  kind: string;
  step_id?: string;
  tool?: string;
  reason?: string;
  detail?: string;
  outcome?: string;
}

/** A page of the decision log, newest first. */
export interface SchedulerDecisionPage {
  entries: SchedulerDecision[];
  /** The `before` of the page before this one. */
  cursor: number;
  more: boolean;
}

/** What `scheduler explain` says about one step (`Explanation`), as much of it as the view shows. */
export interface SchedulerExplanation {
  standing: string;
  reason: string;
  detail?: string;
  retry_at_ms?: number;
}

const words = (code: string) => code.replaceAll('_', ' ');
const sentence = (text: string) => text.charAt(0).toUpperCase() + text.slice(1);

/** Why the scheduler did not place work, as a phrase. */
const REASONS: Readonly<Record<string, string>> = {
  target_cap: 'the project’s session limit is reached',
  machine_refused: 'its machine has no room',
  no_tool_available: 'no AI tool account can take it now',
  resource_held: 'a resource it needs is held',
  resource_unknown: 'a resource’s claims could not be read',
  cooldown: 'its project is cooling down after a recent start',
  refusal_backoff: 'its machine refused recently and is asked again later',
  policy_denied: 'the policy does not allow it',
  target_unavailable: 'its project could not be reached',
  provisional_not_dispatched: 'provisional work is not started under this policy',
  speculation_refused: 'the provisional rule refuses it',
  permission_required: 'it waits for a person’s permission',
  actor_required: 'the scheduler has not looked at it yet',
};

export function schedulerReasonText(code: string): string {
  return REASONS[code] ?? words(code);
}

const STANDINGS: Readonly<Record<string, string>> = {
  start: 'It would start on the next pass',
  keep: 'A session keeps it',
  running: 'It is running',
  finished: 'It is finished',
  not_planned: 'It is not in the plan',
  not_ready: 'It is not ready',
};

/** The Why It Is Not Running rows from `scheduler explain`, with the time of the scheduler's last pass. */
export function explanationRows(
  explanation: SchedulerExplanation,
  clock: ClockFormat,
  lastPassMs?: number,
): Array<{ label: string; value: string }> {
  const standing =
    explanation.standing === 'not_placed'
      ? `Waits: ${schedulerReasonText(explanation.reason)}`
      : (STANDINGS[explanation.standing] ?? sentence(words(explanation.standing)));
  const rows = [
    { label: 'Scheduler', value: lastPassMs === undefined ? standing : `${standing} (last pass ${clock(lastPassMs)})` },
  ];
  if (explanation.retry_at_ms !== undefined)
    rows.push({ label: 'Next', value: `Looked at again by ${clock(explanation.retry_at_ms)}` });
  else if (explanation.standing === 'not_placed')
    rows.push({ label: 'Next', value: 'Looked at again when something it waits on changes' });
  return rows;
}

/** A plan step by its ticket's slug on the timeline; a short step id when the timeline does not show it. */
export function stepLabel(stepId: string | undefined, timeline: OrchestrationTimeline | undefined): string {
  if (!stepId) return 'a step';
  const item = timeline?.waves.flatMap((wave) => wave.items).find((candidate) => candidate.step_id === stepId);
  return item?.slug ?? item?.title ?? `step ${stepId.slice(-6)}`;
}

/** One decision as a person reads it: "Started API-12 on Claude", "Deferred DOC-9: a resource it needs is held". */
export function decisionText(decision: SchedulerDecision, timeline: OrchestrationTimeline | undefined): string {
  const step = stepLabel(decision.step_id, timeline),
    dry = decision.mode === 'dry_run';
  switch (decision.kind) {
    case 'tick':
      return dry ? 'Planned a pass (dry run)' : 'Ran a scheduling pass';
    case 'start': {
      if (decision.outcome === 'permission_required') return `${step} waits for permission`;
      const tool = toolLabel(decision.tool);
      return `${dry ? 'Would start' : 'Started'} ${step}${tool ? ` on ${tool}` : ''}`;
    }
    case 'keep':
      return `Kept the session of ${step}${decision.reason ? `: ${words(decision.reason)}` : ''}`;
    case 'wind_down':
      return `${dry ? 'Would wind down' : 'Wound down'} a session${decision.reason ? `: ${words(decision.reason)}` : ''}`;
    case 'unplaced':
      return `Deferred ${step}${decision.reason ? `: ${schedulerReasonText(decision.reason)}` : ''}`;
    case 'assignment':
      return `${step}: ${words(decision.outcome ?? decision.reason ?? 'assignment')}`;
    case 'reconcile':
      return `Reconciled ${step}: ${words(decision.outcome ?? decision.reason ?? 'acted')}`;
    default:
      return `${sentence(words(decision.kind))} ${step}`;
  }
}

/** The overview's Last Pass: when the newest pass ran and what it started and deferred. */
export function lastPassText(page: SchedulerDecisionPage, clock: ClockFormat): string | undefined {
  const tick = page.entries.find((entry) => entry.kind === 'tick');
  if (!tick) return undefined;
  const of = (kind: string) =>
    page.entries.filter((entry) => entry.tick_id === tick.tick_id && entry.kind === kind).length;
  const started = of('start'),
    deferred = of('unplaced');
  return `${clock(tick.at_ms)} · started ${started}, deferred ${deferred}${tick.mode === 'dry_run' ? ' (dry run)' : ''}`;
}

/** The latest decisions the overview lists: decisions about work, not the pass records themselves. */
export function latestDecisions(page: SchedulerDecisionPage, count = 3): SchedulerDecision[] {
  return page.entries.filter((entry) => entry.kind !== 'tick').slice(0, count);
}

/** Read a `scheduler decisions` page (newest first) leniently: a malformed entry is left out. */
export function parseDecisionPage(value: Record<string, unknown>): SchedulerDecisionPage {
  const entries = Array.isArray(value.entries) ? (value.entries as Array<Record<string, unknown>>) : [];
  const text = (field: unknown) => (typeof field === 'string' ? field : undefined);
  return {
    cursor: typeof value.cursor === 'number' ? value.cursor : 0,
    more: value.more === true,
    entries: entries.flatMap((entry) => {
      const record = (entry.record ?? entry) as Record<string, unknown>;
      if (typeof entry.seq !== 'number' || typeof record.at_ms !== 'number' || typeof record.kind !== 'string')
        return [];
      return [
        {
          seq: entry.seq,
          at_ms: record.at_ms,
          tick_id: text(record.tick_id) ?? '',
          mode: text(record.mode) ?? 'applied',
          kind: record.kind,
          step_id: text(record.step_id),
          tool: text(record.tool),
          reason: text(record.reason),
          detail: text(record.detail),
          outcome: text(record.outcome),
        },
      ];
    }),
  };
}

/** Read `scheduler explain`, or undefined when it is not that shape. */
export function parseExplanation(value: Record<string, unknown> | undefined): SchedulerExplanation | undefined {
  if (!value || typeof value.standing !== 'string' || typeof value.reason !== 'string') return undefined;
  return {
    standing: value.standing,
    reason: value.reason,
    detail: typeof value.detail === 'string' ? value.detail : undefined,
    retry_at_ms: typeof value.retry_at_ms === 'number' ? value.retry_at_ms : undefined,
  };
}
