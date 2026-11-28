import type { LiveCi, LiveStoreState } from './api';

/**
 * The legacy display window for a server that predates `verify_by_ms`. The current server omits
 * unverifiable active runs and uses `null` for the deadline of visible runs (HS2-D3JJ49).
 */
export const CI_TRUST_MS = 2 * 60 * 60 * 1000;

function inFlight(ci: Pick<LiveCi, 'status'>): boolean {
  return ci.status === 'in_progress' || ci.status === 'queued';
}

/**
 * Whether glance surfaces show `ci` at `nowMs` (HS2-D3JJ49): a run nothing can verify is never shown, and
 * neither is a run in flight past its `verify_by_ms` (an older server's report: 2 h after it was reported).
 * A transient run stays shown with its last known status; a finished run always shows.
 */
export function ciRunShown(
  ci: Pick<LiveCi, 'status' | 'reported_at_ms' | 'verification' | 'verify_by_ms'>,
  nowMs: number,
): boolean {
  if (ci.status === 'unavailable' || ci.verification === 'unverifiable') return false;
  if (!inFlight(ci)) return true;
  const by =
    ci.verify_by_ms === undefined
      ? ci.reported_at_ms === null
        ? null
        : ci.reported_at_ms + CI_TRUST_MS
      : ci.verify_by_ms;
  return by === null || nowMs <= by;
}

/** When a run was reported, else finished, else started: the newer run of two is the later one. */
function reportedAt(ci: LiveCi): number {
  return ci.reported_at_ms ?? ci.finished_at_ms ?? ci.started_at_ms ?? 0;
}

/**
 * The CI runs `states` show at `nowMs` (HS2-D3JJ49): each store's `ci_runs`, or its `ci` from an older
 * server, only the shown ones, the newest per branch and workflow across the stores, newest first.
 */
export function shownCiRuns(states: readonly LiveStoreState[], nowMs: number): LiveCi[] {
  const newest = new Map<string, LiveCi>();
  for (const ci of states.flatMap((state) => state.ci_runs ?? (state.ci ? [state.ci] : []))) {
    if (!ciRunShown(ci, nowMs)) continue;
    const key = JSON.stringify([ci.branch, ci.workflow]),
      kept = newest.get(key);
    if (!kept || reportedAt(ci) > reportedAt(kept)) newest.set(key, ci);
  }
  return [...newest.values()].sort((a, b) => reportedAt(b) - reportedAt(a));
}

/**
 * The query of `DELETE /checkouts/{reference}/ci-status` that clears one shown run (HS2-7PSBHQ, HS2-D3JJ49):
 * the time it was reported, and its branch and workflow among the checkout's runs.
 */
export function ciClearQuery(reportedAtMs: number, run: { branch?: string; workflow?: string } = {}): string {
  const query = new URLSearchParams({ reported_at_ms: String(Math.floor(reportedAtMs)) });
  if (run.branch !== undefined) query.set('branch', run.branch);
  if (run.workflow !== undefined) query.set('workflow', run.workflow);
  return query.toString();
}
