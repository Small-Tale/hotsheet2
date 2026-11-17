/**
 * HS2-F1BKQ0: the orchestration plan's state when every plan read fails (docs/26 §26.15 "Where the plan
 * lives"): a conflict between this machine's pre-sharing definitions and the checked-in plan, or a damaged
 * plan file. Pure. It reads `readiness health` and says what a person can do about it.
 */

/** The codes a timeline read fails with when the plan itself cannot be read. */
export const PLAN_FAILURE_CODES: ReadonlySet<string> = new Set([
  'plan_conflict',
  'corrupt_orchestration_data',
  'upgrade_required',
]);

export interface PlanHealth {
  state: 'ok' | 'conflict' | 'damaged';
  file?: 'checked_in' | 'local';
  reason?: string;
  conflicts: string[];
  local: string[];
  discard_resolves: boolean;
}

const strings = (value: unknown) => (Array.isArray(value) ? value.filter((item) => typeof item === 'string') : []);

/** Read `readiness health`; undefined when it is not that shape. */
export function parsePlanHealth(value: Record<string, unknown> | undefined): PlanHealth | undefined {
  const state = value?.state;
  if (state !== 'ok' && state !== 'conflict' && state !== 'damaged') return undefined;
  return {
    state,
    file: value?.file === 'checked_in' || value?.file === 'local' ? value.file : undefined,
    reason: typeof value?.reason === 'string' ? value.reason : undefined,
    conflicts: strings(value?.conflicts),
    local: strings(value?.local),
    discard_resolves: value?.discard_resolves === true,
  };
}

/** A step by its slug on the last timeline read, else the end of its id. */
export type StepName = (stepId: string) => string;

const list = (ids: readonly string[], name: StepName) => ids.map(name).join(', ');

/** What the view says about a plan it cannot read, and whether Keep the Checked-In Plan… resolves it. */
export function planProblem(
  health: PlanHealth,
  name: StepName,
): { title: string; detail: string; canDiscard: boolean } | undefined {
  if (health.state === 'ok') return undefined;
  if (health.state === 'damaged') {
    const newer = health.reason === 'unsupported_version';
    return health.file === 'local'
      ? {
          title: 'This Machine’s Plan Records Are Damaged',
          detail: newer
            ? 'A newer version of Hot Sheet wrote this machine’s plan records. Update Hot Sheet on this machine.'
            : 'This machine’s plan records cannot be read, so nothing is planned or placed here. They are never overwritten; restore them from a backup.',
          canDiscard: false,
        }
      : {
          title: 'The Checked-In Plan Cannot Be Read',
          detail: newer
            ? '.hotsheet2/orchestration-plan.json was written by a newer version of Hot Sheet. Update Hot Sheet on this machine.'
            : '.hotsheet2/orchestration-plan.json is damaged, for example by merge markers a Git merge left. Resolve the merge in Git and commit the file; the view reads it again then.',
          canDiscard: false,
        };
  }
  if (health.discard_resolves)
    return {
      title: 'This Machine’s Plan Differs From the Checked-In Plan',
      detail: `This machine kept ${list(health.local, name)} from before the plan was checked in, and the checked-in plan has ${
        health.local.length === 1 ? 'it' : 'them'
      } differently (steps involved: ${list(health.conflicts, name)}). Keep the checked-in plan to drop this machine’s copies, or change the checked-in plan in Git first.`,
      canDiscard: true,
    };
  return {
    title: 'The Plan Has Conflicting Steps',
    detail: `Steps ${list(health.conflicts, name)} conflict${
      health.local.length > 0 ? ', and dropping this machine’s copies would not resolve it' : ' in the checked-in plan'
    }. Resolve them in .hotsheet2/orchestration-plan.json in Git; the view reads it again then.`,
    canDiscard: false,
  };
}
