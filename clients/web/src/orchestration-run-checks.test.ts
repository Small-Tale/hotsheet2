import { describe, expect, it } from 'vitest';

import {
  dispositionText,
  parseRunReconciliation,
  reconciliationRows,
  runEventText,
  runReasonText,
} from './orchestration-run-checks';

const clock = (ms: number) => `t${ms}`;

describe('a run’s last reconciliation in words (HS2-XM692S)', () => {
  it('names every disposition of docs/26 §26.14 and reads a newer one as words', () => {
    for (const code of [
      'continue',
      'wait_for_dispatcher',
      'retry_dispatch',
      'retry_cancel',
      'needs_attention',
      'hold_unverified',
      'paused',
      'finished',
    ])
      expect(dispositionText(code)).not.toContain('_');
    expect(dispositionText('needs_attention')).toBe('Needs a person: the record and the project disagree');
    expect(dispositionText('some_new_state')).toBe('Some new state');
  });

  it('says when nothing is stored, then Checked, Not verified and Differs as they apply', () => {
    expect(reconciliationRows(undefined, clock)).toEqual([
      { label: 'Checked', value: 'Not checked since its last change' },
    ]);
    expect(reconciliationRows({ at_ms: 5, disposition: 'continue', findings: [] }, clock)).toEqual([
      { label: 'Checked', value: 'The run’s worker holds its ticket, since t5' },
    ]);
    const rows = reconciliationRows(
      {
        at_ms: 7,
        disposition: 'hold_unverified',
        findings: [
          { source: 'target_ticket', verification: 'verified' },
          { source: 'ci_run', verification: 'unverifiable', reason: 'provider_unreachable' },
          { source: 'session', verification: 'unverifiable', reason: 'source_not_wired' },
          { source: 'new_source', verification: 'unverifiable' },
          { source: 'target_claim', verification: 'mismatched', reason: 'unrecorded_live_work' },
        ],
      },
      clock,
    );
    expect(rows).toEqual([
      { label: 'Checked', value: 'Held: something it depends on could not be verified, since t7' },
      { label: 'Not verified', value: 'CI (provider unreachable), new source' },
      { label: 'Differs', value: 'Claim (unrecorded live work)' },
    ]);
  });

  it('says why a CI watch is not running in plain language, with the next step when it was given up (HS2-507DGZ)', () => {
    const rows = reconciliationRows(
      {
        at_ms: 9,
        disposition: 'hold_unverified',
        findings: [{ source: 'ci_run', verification: 'unverifiable', reason: 'watch_gone' }],
      },
      clock,
    );
    expect(rows[1]).toEqual({
      label: 'Not verified',
      value:
        'CI (the CI provider kept ending the watch; check the provider’s credentials and repository, then link the gate again)',
    });
    for (const reason of ['watch_lost', 'watch_ended', 'watch_gone']) {
      const [, row] = reconciliationRows(
        {
          at_ms: 1,
          disposition: 'hold_unverified',
          findings: [{ source: 'ci_run', verification: 'unverifiable', reason }],
        },
        clock,
      );
      expect(row.value).not.toContain('_');
    }
  });

  it('reads the record’s reconciliation and ignores a missing or malformed one', () => {
    expect(parseRunReconciliation(undefined)).toBeUndefined();
    expect(parseRunReconciliation({ run_id: 'r' })).toBeUndefined();
    expect(parseRunReconciliation({ reconciliation: { at_ms: 'x', disposition: 'continue' } })).toBeUndefined();
    expect(
      parseRunReconciliation({
        reconciliation: {
          at_ms: 3,
          disposition: 'continue',
          findings: [{ source: 'ci_run', verification: 'verified' }, { bogus: true }],
        },
      }),
    ).toEqual({ at_ms: 3, disposition: 'continue', findings: [{ source: 'ci_run', verification: 'verified' }] });
    expect(parseRunReconciliation({ reconciliation: { at_ms: 3, disposition: 'paused' } })?.findings).toEqual([]);
  });
});

describe('a released or returned run in words (HS2-ESVYQH)', () => {
  it('names the reasons a run changes and reads a newer one as words', () => {
    for (const code of [
      'claim_released',
      'claim_taken',
      'claim_confirmed',
      'claimed_for_session',
      'assigned_by_dispatcher',
      'target_completed',
    ])
      expect(runReasonText(code)).not.toContain('_');
    expect(runReasonText('claim_released')).toBe(
      'its worker let the ticket go: a handoff, or a return for corrections',
    );
    expect(runReasonText('some_new_reason')).toBe('some new reason');
  });

  it('writes each event row: a release, a claim made again after a return, and one learned from the project', () => {
    expect(runEventText({ kind: 'state', state: 'intervened', reason: 'claim_released', source: 'target' })).toBe(
      'State: released by its worker (its worker let the ticket go: a handoff, or a return for corrections) · learned from the project',
    );
    expect(runEventText({ kind: 'state', state: 'dispatched', reason: 'claimed_for_session' })).toBe(
      'State: dispatched (claimed for its session, with no dispatcher to assign it)',
    );
    expect(runEventText({ kind: 'session' })).toBe('Session');
    expect(runEventText({ kind: 'session_request', reason: 'session_request_delivered' })).toBe(
      'Session request (session request delivered)',
    );
  });
});
