/**
 * HS2-9XJEZC: the validation steps of one Orchestration view card (docs/26 §26.12) for the card
 * inspector's Validation section (design HS2-2XJSJK, answer A: one Run Validation that runs every step
 * not passed). Pure. Output is never read or shown: only states, codes, times, exit codes and digests.
 */

/** One step's latest evidence, as the inspector shows it. */
export interface ValidationStepView {
  step_id: string;
  /** `command` or `profile`: a command step asks a person before each run. */
  check?: string;
  state: string;
  reason?: string;
  /** When the latest attempt finished, else when it started. */
  at_ms?: number;
  exit_code?: number;
  /** The first characters of the standard output's digest: a reference, never the output. */
  evidence?: string;
}

/** A Run Validation request that waits on a person's answer to the orchestrator's permission request. */
export interface PendingValidation {
  step_id: string;
  operation_id: string;
  request_id: string;
}

const STATES: Readonly<Record<string, string>> = {
  running: 'Running',
  passed: 'Passed',
  failed: 'Failed',
  skipped: 'Skipped',
  unavailable: 'Could not run',
  unverified: 'Not confirmed',
  cancelled: 'Cancelled',
};

const words = (code: string) => code.replaceAll('_', ' ');

/** Read one entry of `validations/list` (a `ValidationEvidence`); undefined for another target's step. */
export function parseValidationStep(
  evidence: Record<string, unknown>,
  targetId: string,
): ValidationStepView | undefined {
  const step = (evidence.step ?? {}) as Record<string, unknown>,
    definition = (step.definition ?? {}) as Record<string, unknown>;
  if (typeof definition.target_id === 'string' && definition.target_id !== targetId) return undefined;
  const latest = (evidence.latest ?? undefined) as Record<string, unknown> | undefined,
    outcome = (latest?.outcome ?? undefined) as Record<string, unknown> | undefined,
    stdout = (outcome?.stdout ?? undefined) as Record<string, unknown> | undefined,
    check = (definition.check ?? undefined) as Record<string, unknown> | undefined;
  const number = (value: unknown) => (typeof value === 'number' ? value : undefined);
  return {
    step_id: typeof definition.step_id === 'string' ? definition.step_id : 'step',
    check: typeof check?.kind === 'string' ? check.kind : undefined,
    state: typeof evidence.state === 'string' ? evidence.state : 'unverified',
    reason: typeof evidence.reason === 'string' ? evidence.reason : undefined,
    at_ms: number(latest?.finished_at_ms) ?? number(latest?.created_at_ms),
    exit_code: number(outcome?.exit_code),
    evidence: typeof stdout?.sha256 === 'string' ? stdout.sha256.slice(0, 7) : undefined,
  };
}

/** The row label: the step's name, marked when it is a command the orchestration project chose. */
export function validationStepLabel(step: ValidationStepView): string {
  return step.check === 'command' ? `${step.step_id} (command)` : step.step_id;
}

/** "Passed · 14:58 · exit 0 · evidence 4f2a91c", "Skipped · prerequisite not passed", "Waits for permission". */
export function validationStepValue(
  step: ValidationStepView,
  clock: (ms: number) => string,
  pending?: PendingValidation,
): string {
  if (pending && step.state !== 'running') return 'Waits for permission';
  const parts = [STATES[step.state] ?? words(step.state)];
  if (step.reason) parts.push(words(step.reason));
  if (step.at_ms !== undefined && step.state !== 'skipped') parts.push(clock(step.at_ms));
  if (step.exit_code !== undefined) parts.push(`exit ${step.exit_code}`);
  if (step.evidence) parts.push(`evidence ${step.evidence}`);
  return parts.join(' · ');
}

/** The steps Run Validation runs: every step not passed and not already running, in definition order. */
export function stepsToRun(steps: readonly ValidationStepView[]): ValidationStepView[] {
  return steps.filter((step) => step.state !== 'passed' && step.state !== 'running');
}
