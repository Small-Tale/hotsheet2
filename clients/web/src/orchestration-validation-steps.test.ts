import { describe, expect, it } from 'vitest';

import {
  parseValidationStep,
  stepsToRun,
  validationStepLabel,
  validationStepValue,
  type ValidationStepView,
} from './orchestration-validation-steps';

const clock = (ms: number) => `t${ms}`;

describe('validation steps in the card inspector (HS2-9XJEZC)', () => {
  it('reads a step from its definition, keeps references only, and skips another target’s step', () => {
    const evidence = {
      step: {
        definition: { step_id: 'smoke', target_id: 'api', check: { kind: 'command', program: 'true', args: [] } },
      },
      state: 'failed',
      reason: 'exit_code_mismatch',
      latest: {
        created_at_ms: 10,
        finished_at_ms: 20,
        outcome: {
          end: 'exited',
          exit_code: 1,
          started_at_ms: 11,
          duration_ms: 9,
          stdout: { sha256: 'abcdef0123', bytes: 3 },
        },
      },
    };
    expect(parseValidationStep(evidence, 'api')).toEqual({
      step_id: 'smoke',
      check: 'command',
      state: 'failed',
      reason: 'exit_code_mismatch',
      at_ms: 20,
      exit_code: 1,
      evidence: 'abcdef0',
    });
    expect(parseValidationStep(evidence, 'web')).toBeUndefined();
    // A step that never ran: unverified, no time, no exit code, no evidence.
    expect(parseValidationStep({ step: { definition: { step_id: 'lint' } }, state: 'unverified' }, 'api')).toEqual({
      step_id: 'lint',
      check: undefined,
      state: 'unverified',
      reason: undefined,
      at_ms: undefined,
      exit_code: undefined,
      evidence: undefined,
    });
    const running = parseValidationStep(
      { step: { definition: { step_id: 'e2e' } }, state: 'running', latest: { created_at_ms: 5 } },
      'api',
    );
    expect(running?.at_ms).toBe(5);
    expect(parseValidationStep({}, 'api')).toMatchObject({ step_id: 'step', state: 'unverified' });
  });

  it('says each state with its reason, time, exit code and digest, and a wait for permission', () => {
    const step = (rest: Partial<ValidationStepView>): ValidationStepView => ({
      step_id: 's',
      state: 'passed',
      ...rest,
    });
    expect(validationStepValue(step({ at_ms: 1, exit_code: 0, evidence: 'abc1234' }), clock)).toBe(
      'Passed · t1 · exit 0 · evidence abc1234',
    );
    expect(validationStepValue(step({ state: 'skipped', reason: 'prerequisite_not_passed', at_ms: 1 }), clock)).toBe(
      'Skipped · prerequisite not passed',
    );
    expect(validationStepValue(step({ state: 'unavailable', reason: 'program_not_found' }), clock)).toBe(
      'Could not run · program not found',
    );
    expect(validationStepValue(step({ state: 'new_state' }), clock)).toBe('new state');
    const pending = { step_id: 's', operation_id: 'op', request_id: 'r' };
    expect(validationStepValue(step({ state: 'failed' }), clock, pending)).toBe('Waits for permission');
    // Once it runs, the run's own state shows.
    expect(validationStepValue(step({ state: 'running', at_ms: 2 }), clock, pending)).toBe('Running · t2');
    expect(validationStepLabel(step({ check: 'command' }))).toBe('s (command)');
    expect(validationStepLabel(step({ check: 'profile' }))).toBe('s');
  });

  it('runs every step not passed and not running, in order, and nothing when all passed', () => {
    const steps: ValidationStepView[] = ['passed', 'failed', 'running', 'unverified', 'skipped', 'cancelled'].map(
      (state, index) => ({ step_id: `s${index}`, state }),
    );
    expect(stepsToRun(steps).map((step) => step.step_id)).toEqual(['s1', 's3', 's4', 's5']);
    expect(stepsToRun([{ step_id: 'a', state: 'passed' }])).toEqual([]);
    expect(stepsToRun([])).toEqual([]);
  });
});
