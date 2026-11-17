/**
 * HS2-0D60T8: work built ahead of a pending gate (docs/26 §26.22) for the card inspector's Built Ahead
 * section (design HS2-2XJSJK, answer A). Pure. It reads the `provisional` and `speculation` members of
 * `readiness show` for the selected step; slugs come from the timeline the view already holds.
 */
import type { OrchestrationTimeline } from './orchestration-timeline';

export interface ProvisionalGateView {
  target_id: string;
  connection_id: string;
  native_id: string;
  commit: string;
  /** `pending`, `green`, `red`, `superseded`, `cancelled` or `unknown`. */
  result: string;
}

export interface BuiltAheadView {
  /** `provisional`, `confirmed`, `at_risk` or `reopened`. */
  state: string;
  depth: number;
  /** The smallest depth limit of the policies involved, when the rule's answer was read. */
  max_depth?: number;
  gates: ProvisionalGateView[];
  /** `rebase_onto_fix` or `reverify_by_covering_run`, for at-risk work. */
  recovery?: string;
  reopen_reason?: string;
  /** It must stay off the target's shipping branch now. */
  held: boolean;
}

const text = (value: unknown) => (typeof value === 'string' ? value : undefined);

/** Read `readiness show`'s `provisional` (the recorded work), with `speculation.max_depth` when present. */
export function parseBuiltAhead(step: Record<string, unknown> | undefined): BuiltAheadView | undefined {
  const work = step?.provisional as Record<string, unknown> | undefined;
  if (!work || typeof work.state !== 'string') return undefined;
  const standing = step?.speculation as Record<string, unknown> | undefined;
  const gates = Array.isArray(work.gates) ? (work.gates as Array<Record<string, unknown>>) : [];
  return {
    state: work.state,
    depth: typeof work.depth === 'number' ? work.depth : 0,
    max_depth: typeof standing?.max_depth === 'number' ? standing.max_depth : undefined,
    gates: gates.map((gate) => ({
      target_id: text(gate.target_id) ?? '',
      connection_id: text(gate.connection_id) ?? '',
      native_id: text(gate.native_id) ?? '',
      commit: text(gate.commit) ?? '',
      result: text(gate.result) ?? 'unknown',
    })),
    recovery: text(work.recovery),
    reopen_reason: text(work.reopen_reason),
    held: work.held === true,
  };
}

/** The prerequisite ticket a gate belongs to, by its slug on the timeline, else its project's label. */
export function gateTicketLabel(gate: ProvisionalGateView, timeline: OrchestrationTimeline): string {
  const items = timeline.waves.flatMap((wave) => wave.items);
  const match = items.find(
    (item) =>
      item.target.target_id === gate.target_id &&
      item.target.connection_id === gate.connection_id &&
      item.target.native_id === gate.native_id,
  );
  if (match?.slug) return match.slug;
  const waited = items
    .flatMap((item) => item.waits_on ?? [])
    .find((wait) => wait.native_id === gate.native_id && wait.connection_id === gate.connection_id);
  if (waited?.slug) return waited.slug;
  const project = timeline.targets.find((target) => target.target_id === gate.target_id)?.label;
  return project ? `a ${project} ticket` : 'a prerequisite';
}

const RESULTS: Readonly<Record<string, string>> = {
  pending: 'CI running',
  green: 'CI passed',
  red: 'CI failed',
  superseded: 'CI superseded',
  cancelled: 'CI cancelled',
  unknown: 'CI result unknown',
};

const STATES: Readonly<Record<string, string>> = {
  provisional: 'Provisional',
  confirmed: 'Confirmed',
  at_risk: 'At risk',
  reopened: 'Reopened',
};

const words = (code: string) => code.replaceAll('_', ' ');

/** The Built Ahead rows: Rests on (one per gate), State, Depth, and Held when it must stay off main. */
export function builtAheadRows(
  view: BuiltAheadView,
  timeline: OrchestrationTimeline,
): Array<{ label: string; value: string }> {
  const rows = view.gates.map((gate) => ({
    label: 'Rests on',
    value: `${gateTicketLabel(gate, timeline)} · ${RESULTS[gate.result] ?? words(gate.result)} on ${gate.commit.slice(0, 7)}`,
  }));
  if (rows.length === 0) rows.push({ label: 'Rests on', value: 'No pending gate' });
  rows.push({
    label: 'State',
    value: `${STATES[view.state] ?? words(view.state)}${view.reopen_reason ? ` (${words(view.reopen_reason)})` : ''}`,
  });
  rows.push({
    label: 'Depth',
    value: view.max_depth === undefined ? String(view.depth) : `${view.depth} of ${view.max_depth} allowed`,
  });
  if (view.held) rows.push({ label: 'Shipping', value: 'Held off the shipping branch for now' });
  return rows;
}

/** The At Risk banner's detail: what to repair first and how this work recovers after. */
export function atRiskDetail(view: BuiltAheadView, timeline: OrchestrationTimeline): string {
  const failed = view.gates.filter((gate) => gate.result === 'red').map((gate) => gateTicketLabel(gate, timeline));
  const first = failed.length > 0 ? failed.join(' and ') : 'the prerequisite';
  const after =
    view.recovery === 'reverify_by_covering_run'
      ? 'the next covering CI run checks this work again'
      : 'this work is rebased onto the fix and checked again';
  return `${first === 'the prerequisite' ? 'A prerequisite’s' : `${first}’s`} CI failed. Repair ${first} first; after that ${after}.`;
}

/** Whether a person may reopen it: provisional or at-risk work; confirmed and reopened work cannot be. */
export function canReopen(view: BuiltAheadView): boolean {
  return view.state === 'provisional' || view.state === 'at_risk';
}

/** The reason codes Reopen… offers: why the fix invalidated the work (stable codes, never prose). */
export const REOPEN_REASONS: ReadonlyArray<{ code: string; label: string }> = [
  { code: 'fix_changed_contract', label: 'The Fix Changed the Contract…' },
  { code: 'fix_changed_behavior', label: 'The Fix Changed the Behavior…' },
  { code: 'work_needs_redo', label: 'The Work Needs to Be Redone…' },
];
