/**
 * HS2-XM692S: a run's last reconciliation (docs/26 §26.14) in words, for the Run section of the
 * Orchestration view's card inspector (design HS2-2XJSJK). Pure; codes it does not know read as words.
 */

export interface RunFinding {
  source: string;
  verification: string;
  reason?: string;
}

/** The outcome a run keeps from its last reconciliation, as `GET .../runs/{run}` answers it. */
export interface RunReconciliation {
  at_ms: number;
  disposition: string;
  findings: RunFinding[];
}

const DISPOSITIONS: Readonly<Record<string, string>> = {
  continue: 'The run’s worker holds its ticket',
  wait_for_dispatcher: 'Waits for the project’s dispatcher to assign it',
  retry_dispatch: 'Nothing holds its ticket; the dispatch is repeated',
  retry_cancel: 'The cancel did not reach the project; it is repeated',
  needs_attention: 'Needs a person: the record and the project disagree',
  hold_unverified: 'Held: something it depends on could not be verified',
  paused: 'Paused by a person',
  finished: 'Finished',
};

const SOURCES: Readonly<Record<string, string>> = {
  target_reachability: 'Project',
  target_ticket: 'Ticket',
  target_claim: 'Claim',
  run_lease: 'Lease',
  ci_run: 'CI',
  deployment: 'Deployment',
  validation: 'Validation',
  session: 'Session',
};

const words = (code: string) => code.replaceAll('_', ' ');

/**
 * HS2-507DGZ: why a CI gate's watch is not running, in plain language (docs/26 §26.13), with the next step
 * where a person or an agent must act.
 */
export const CI_WATCH_REASONS: Readonly<Record<string, string>> = {
  not_watched: 'nothing watches it yet; link the gate again to start its watch',
  watch_lost: 'the watch was interrupted; it resumes when the job monitor runs again',
  watch_ended: 'the CI provider ended the watch; it is linked again automatically',
  watch_gone:
    'the CI provider kept ending the watch; check the provider’s credentials and repository, then link the gate again',
};

/**
 * HS2-A7YCB2: the unknown CI gate reasons a person may answer with Link Again: nothing watches the gate
 * (`not_watched`), the provider ended its watch (`watch_ended`) or kept ending it (`watch_gone`).
 */
export const RELINKABLE_WATCH_REASONS: ReadonlySet<string> = new Set(['not_watched', 'watch_ended', 'watch_gone']);

/** The body of `POST .../runs/{run}/gates` that links a gate again: the same commit and CI scope. */
export interface GateLinkRequest {
  commit: string;
  provider?: string;
  repository?: string;
  workflow?: string;
  branch?: string;
  artifact?: string;
}

/**
 * HS2-A7YCB2: the request that links a standing CI gate again, as `GET .../gates` answers the gate, or
 * undefined when the gate is not an unknown CI gate whose watch a link starts again (docs/26 §26.13).
 * Linking the same commit and scope returns the same gate and watches it again.
 */
export function relinkRequest(gate: Record<string, unknown>): GateLinkRequest | undefined {
  const text = (value: unknown) => (typeof value === 'string' && value !== '' ? value : undefined);
  if (gate.kind !== 'ci' || gate.state !== 'unknown' || gate.superseded_by !== undefined) return undefined;
  const reason = text(gate.reason),
    commit = text(gate.commit);
  if (!reason || !RELINKABLE_WATCH_REASONS.has(reason) || !commit) return undefined;
  const scope = (typeof gate.ci === 'object' && gate.ci !== null ? gate.ci : {}) as Record<string, unknown>;
  const request: GateLinkRequest = { commit };
  const provider = text(gate.provider),
    repository = text(scope.repository),
    workflow = text(scope.workflow),
    branch = text(scope.branch),
    artifact = text(scope.artifact);
  if (provider) request.provider = provider;
  if (repository) request.repository = repository;
  if (workflow) request.workflow = workflow;
  if (branch) request.branch = branch;
  if (artifact) request.artifact = artifact;
  return request;
}

export function dispositionText(disposition: string): string {
  const known = DISPOSITIONS[disposition];
  if (known) return known;
  const text = words(disposition);
  return text.charAt(0).toUpperCase() + text.slice(1);
}

const finding = (item: RunFinding) =>
  `${SOURCES[item.source] ?? words(item.source)}${item.reason ? ` (${CI_WATCH_REASONS[item.reason] ?? words(item.reason)})` : ''}`;

/**
 * The Run section's reconciliation rows: Checked, then Not verified and Differs when a source is so.
 * A source the orchestrator does not read yet (`source_not_wired`) holds nothing back and is left out.
 */
export function reconciliationRows(
  reconciliation: RunReconciliation | undefined,
  clock: (ms: number) => string,
): Array<{ label: string; value: string }> {
  if (!reconciliation) return [{ label: 'Checked', value: 'Not checked since its last change' }];
  const rows = [
    { label: 'Checked', value: `${dispositionText(reconciliation.disposition)}, since ${clock(reconciliation.at_ms)}` },
  ];
  const unverified = reconciliation.findings.filter(
    (item) => item.verification === 'unverifiable' && item.reason !== 'source_not_wired',
  );
  if (unverified.length > 0) rows.push({ label: 'Not verified', value: unverified.map(finding).join(', ') });
  const mismatched = reconciliation.findings.filter((item) => item.verification === 'mismatched');
  if (mismatched.length > 0) rows.push({ label: 'Differs', value: mismatched.map(finding).join(', ') });
  return rows;
}

/** The record's `reconciliation`, or undefined when absent or not the documented shape. */
export function parseRunReconciliation(run: Record<string, unknown> | undefined): RunReconciliation | undefined {
  const value = run?.reconciliation as Record<string, unknown> | undefined;
  if (!value || typeof value.at_ms !== 'number' || typeof value.disposition !== 'string') return undefined;
  const findings = Array.isArray(value.findings) ? (value.findings as Array<Record<string, unknown>>) : [];
  return {
    at_ms: value.at_ms,
    disposition: value.disposition,
    findings: findings
      .filter((item) => typeof item.source === 'string' && typeof item.verification === 'string')
      .map((item) => ({
        source: item.source as string,
        verification: item.verification as string,
        reason: typeof item.reason === 'string' ? item.reason : undefined,
      })),
  };
}

/** HS2-ESVYQH: why a run changed, in words; codes it does not know read as words. */
const RUN_REASONS: Readonly<Record<string, string>> = {
  claim_released: 'its worker let the ticket go: a handoff, or a return for corrections',
  claim_taken: 'someone else took the ticket',
  claim_confirmed: 'its claim was confirmed',
  claimed_for_session: 'claimed for its session, with no dispatcher to assign it',
  assigned_by_dispatcher: 'assigned by the project’s dispatcher',
  target_completed: 'its ticket was completed',
  // HS2-A7YCB2: a gate event's reason, shorter than the Gates row that says what to do.
  not_watched: 'not watched yet',
  watch_lost: 'its watch was interrupted',
  watch_ended: 'the CI provider ended its watch',
  watch_gone: 'the CI provider kept ending its watch',
};

export function runReasonText(reason: string): string {
  return RUN_REASONS[reason] ?? words(reason);
}

/** One row of the Run section's events: its kind, the state it reached and why. */
export function runEventText(event: { kind: string; state?: string; reason?: string; source?: string }): string {
  const kind = words(event.kind);
  const state = event.state === 'intervened' ? 'released by its worker' : event.state && words(event.state);
  return `${kind.charAt(0).toUpperCase()}${kind.slice(1)}${state ? `: ${state}` : ''}${
    event.reason ? ` (${runReasonText(event.reason)})` : ''
  }${event.source === 'target' ? ' · learned from the project' : ''}`;
}
