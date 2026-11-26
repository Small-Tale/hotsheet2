import { describe, expect, it } from 'vitest';

import {
  dispositionText,
  parseRunReconciliation,
  reconciliationRows,
  RELINKABLE_WATCH_REASONS,
  relinkRequest,
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

describe('Link Again on an unknown CI gate (HS2-A7YCB2)', () => {
  const commit = 'a'.repeat(40);
  const gate = (extra: Record<string, unknown> = {}) => ({
    gate_id: 'gate-1',
    kind: 'ci',
    provider: 'github-actions',
    commit,
    ci: { repository: 'acme/synthetic', workflow: 'CI', branch: 'main' },
    state: 'unknown',
    reason: 'watch_gone',
    ...extra,
  });

  it('links the same commit and scope again for each reason a link starts the watch again', () => {
    expect([...RELINKABLE_WATCH_REASONS].sort()).toEqual(['not_watched', 'watch_ended', 'watch_gone']);
    for (const reason of RELINKABLE_WATCH_REASONS)
      expect(relinkRequest(gate({ reason }))).toEqual({
        commit,
        provider: 'github-actions',
        repository: 'acme/synthetic',
        workflow: 'CI',
        branch: 'main',
      });
    // The artifact a passed verdict records is kept too.
    expect(
      relinkRequest(gate({ ci: { repository: 'acme/synthetic', workflow: '*', branch: 'trunk', artifact: 'app' } })),
    ).toEqual({
      commit,
      provider: 'github-actions',
      repository: 'acme/synthetic',
      workflow: '*',
      branch: 'trunk',
      artifact: 'app',
    });
    // A gate read without its scope still links its commit; the route fills in the defaults.
    expect(relinkRequest({ kind: 'ci', state: 'unknown', reason: 'not_watched', commit })).toEqual({ commit });
  });

  it('offers nothing for a watched, settled, superseded or other gate, or another unknown reason', () => {
    for (const other of [
      { state: 'pending', reason: undefined },
      { state: 'passed', reason: undefined },
      { state: 'failed', reason: undefined },
      { reason: 'watch_lost' },
      { reason: 'evidence_missing' },
      { reason: undefined },
      { kind: 'deployment' },
      { kind: 'review' },
      { superseded_by: 'gate-2' },
      { commit: undefined },
      { commit: '' },
    ])
      expect(relinkRequest(gate(other)), JSON.stringify(other)).toBeUndefined();
  });

  it('words every reason Link Again answers, so no row reads as a code', () => {
    expect(runEventText({ kind: 'gate', reason: 'watch_gone', source: 'target' })).toBe(
      'Gate (the CI provider kept ending its watch) · learned from the project',
    );
    for (const reason of [...RELINKABLE_WATCH_REASONS, 'watch_lost']) expect(runReasonText(reason)).not.toContain('_');
    for (const reason of RELINKABLE_WATCH_REASONS) {
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
