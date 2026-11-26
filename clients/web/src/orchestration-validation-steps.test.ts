import { describe, expect, it } from 'vitest';

import {
  parseValidationOperation,
  parseValidationRunAnswer,
  parseValidationStep,
  recoveredValidations,
  stepsToRun,
  validationOperation,
  validationRunUnsettled,
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

describe('a waiting Run Validation recovered after a reload (HS2-NY0MRJ)', () => {
  it('reads the card and step back from the operation id, hyphens included', () => {
    const op = validationOperation('01ARZ3NDEKTSV4RRFFQ69G5FAV', 'e2e-login', 'a1b2c3d4e5f6');
    expect(op).toBe('validate-01ARZ3NDEKTSV4RRFFQ69G5FAV-e2e-login-a1b2c3d4e5f6');
    expect(parseValidationOperation(op)).toEqual({ planStep: '01ARZ3NDEKTSV4RRFFQ69G5FAV', stepId: 'e2e-login' });
    expect(parseValidationOperation('sched-run-01ARZ3NDEKTSV4RRFFQ69G5FAV')).toBeUndefined();
    expect(parseValidationOperation('validate-01ARZ-smoke-short')).toBeUndefined();
  });

  it('recovers only this project’s Run Validation requests', () => {
    const ask = (operation_id: string, owner_checkout_id = 'launch') => ({
      request_id: `r-${operation_id}`,
      operation_id,
      owner_checkout_id,
    });
    const op = validationOperation('01ARZ3NDEKTSV4RRFFQ69G5FAV', 'smoke', 'a1b2c3d4e5f6');
    expect(
      recoveredValidations(
        [
          ask(op),
          ask('sched-run-x'),
          ask(validationOperation('01ARZ3NDEKTSV4RRFFQ69G5FAV', 'lint', '000000000000'), 'other'),
        ],
        'launch',
      ),
    ).toEqual([{ step_id: 'smoke', operation_id: op, request_id: `r-${op}` }]);
    expect(recoveredValidations([], 'launch')).toEqual([]);
  });
});

describe('a Run Validation answer (HS2-JGFPG4)', () => {
  it('reads the result state and the attempts left from a ValidationRunView', () => {
    expect(parseValidationRunAnswer({ result: { state: 'running' }, attempts_left: 0 })).toEqual({
      state: 'running',
      attemptsLeft: 0,
    });
    expect(parseValidationRunAnswer({ result: { state: 'failed' }, attempts_left: 2 })).toEqual({
      state: 'failed',
      attemptsLeft: 2,
    });
  });

  it('reads another shape as settled', () => {
    for (const answer of [
      undefined,
      null,
      'running',
      {},
      { result: null, attempts_left: 1.5 },
      { result: { state: 7 }, attempts_left: -1 },
      { result: 'running', attempts_left: '3' },
    ])
      expect(validationRunUnsettled(parseValidationRunAnswer(answer))).toBe(false);
  });

  it('is unsettled while running or while the policy allows another attempt', () => {
    expect(validationRunUnsettled({ state: 'running', attemptsLeft: 0 })).toBe(true);
    expect(validationRunUnsettled({ state: 'failed', attemptsLeft: 1 })).toBe(true);
    expect(validationRunUnsettled({ state: 'failed', attemptsLeft: 0 })).toBe(false);
    expect(validationRunUnsettled({ state: 'passed', attemptsLeft: 0 })).toBe(false);
  });
});
