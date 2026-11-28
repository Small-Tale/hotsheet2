import { describe, expect, it } from 'vitest';

import type { LiveCiStatus } from './api';
import {
  CI_PRESENTATION,
  ciAge,
  ciChipText,
  ciChipTicks,
  ciClearAttributes,
  ciClearItemLabel,
  ciClearTarget,
  ciClearTargetOf,
  ciDetail,
  ciRow,
  ciRunLabel,
  ciRunName,
  ciRunNaming,
  formatCiElapsed,
} from './ci-presentation';
import type { RightNowCi } from './stats-right-now';

const SECOND = 1_000,
  MINUTE = 60 * SECOND,
  HOUR = 60 * MINUTE;

const ci = (status: LiveCiStatus, ageMs?: number): RightNowCi => ({
  status,
  branch: 'main',
  shortSha: '9f8e7d6',
  ...(ageMs === undefined ? {} : { ageMs }),
});

describe('CI elapsed clock face (HS2-505PHC)', () => {
  it('reads m:ss under an hour', () => {
    expect(formatCiElapsed(0)).toBe('0:00');
    expect(formatCiElapsed(5 * SECOND)).toBe('0:05');
    expect(formatCiElapsed(5 * SECOND + 999)).toBe('0:05');
    expect(formatCiElapsed(MINUTE + 5 * SECOND)).toBe('1:05');
    expect(formatCiElapsed(12 * MINUTE)).toBe('12:00');
    expect(formatCiElapsed(HOUR - SECOND)).toBe('59:59');
  });

  it('reads h:mm:ss from an hour', () => {
    expect(formatCiElapsed(HOUR)).toBe('1:00:00');
    expect(formatCiElapsed(HOUR + 2 * MINUTE + 3 * SECOND)).toBe('1:02:03');
    expect(formatCiElapsed(26 * HOUR + 5 * SECOND)).toBe('26:00:05');
  });

  it('reads 0:00 for a start reported ahead of the display clock (clock skew)', () => {
    expect(formatCiElapsed(-1)).toBe('0:00');
    expect(formatCiElapsed(-3 * MINUTE)).toBe('0:00');
  });
});

describe('CI chip face (HS2-505PHC)', () => {
  it('names a running run and its elapsed time', () => {
    expect(ciChipText(ci('in_progress', 5 * SECOND))).toBe('CI · 0:05');
    expect(ciChipText(ci('in_progress', MINUTE + 5 * SECOND))).toBe('CI · 1:05');
    expect(ciChipText(ci('in_progress', HOUR + 2 * MINUTE + 3 * SECOND))).toBe('CI · 1:02:03');
  });

  it('says running when no start was reported', () => {
    expect(ciChipText(ci('in_progress'))).toBe('CI · running');
  });

  it('names queued and failed runs without a clock', () => {
    expect(ciChipText(ci('queued', 30 * SECOND))).toBe('CI · queued');
    expect(ciChipText(ci('queued'))).toBe('CI · queued');
    expect(ciChipText(ci('failure', 12 * MINUTE))).toBe('CI failed');
    expect(ciChipText(ci('failure'))).toBe('CI failed');
  });

  it('has no face for a run the compact line hides', () => {
    for (const status of ['success', 'cancelled', 'unavailable'] as const)
      expect(ciChipText(ci(status, MINUTE))).toBeUndefined();
  });

  it('ticks only while a run with a known start is running', () => {
    expect(ciChipTicks(ci('in_progress', 0))).toBe(true);
    expect(ciChipTicks(ci('in_progress'))).toBe(false);
    for (const status of ['queued', 'failure', 'success', 'cancelled', 'unavailable'] as const)
      expect(ciChipTicks(ci(status, MINUTE))).toBe(false);
  });
});

describe('expanded sidebar row (HS2-57BYEA)', () => {
  it('names the branch, word and commit, with the age trailing', () => {
    expect(ciRow(ci('success', 12 * MINUTE))).toEqual({ label: 'main CI passed · 9f8e7d6', trailing: '12m ago' });
    expect(ciRow(ci('in_progress', 4 * MINUTE))).toEqual({
      label: 'main CI running · 9f8e7d6',
      trailing: 'running 4m',
    });
    expect(ciRow({ status: 'queued', branch: 'main' })).toEqual({ label: 'main CI queued', trailing: '' });
  });

  it('has no word for a run that cannot be verified (HS2-D3JJ49)', () => {
    expect(Object.keys(CI_PRESENTATION).sort()).toEqual(
      ['cancelled', 'failure', 'in_progress', 'queued', 'success', 'unavailable'].sort(),
    );
  });
});

describe('CI detail words (HS2-E16ZEK)', () => {
  it('ages a finished run and words a running or waiting one', () => {
    expect(ciAge(ci('success', 12 * MINUTE))).toBe('12m ago');
    expect(ciAge(ci('in_progress', 4 * MINUTE))).toBe('running 4m');
    expect(ciAge(ci('queued', 2 * MINUTE))).toBe('waiting 2m');
    expect(ciAge(ci('success'))).toBeUndefined();
    expect(ciDetail(ci('success', 12 * MINUTE))).toBe('main CI · 9f8e7d6 · 12m ago');
    expect(ciDetail({ status: 'success', branch: 'main' })).toBe('main CI');
  });
});

describe('several shown runs (HS2-D3JJ49)', () => {
  const named = (workflow: string, branch = 'main'): RightNowCi => ({
    ...ci('in_progress', MINUTE),
    branch,
    workflow,
  });

  it('reads CI for one run, or runs that differ in nothing', () => {
    const one = [named('CI')];
    expect(ciRunNaming(one)).toEqual({ workflows: false, branches: false });
    expect(ciRunName(one[0], ciRunNaming(one))).toBe('CI');
    expect(ciRunNaming([])).toEqual({ workflows: false, branches: false });
    expect(ciChipText(one[0], 'CI')).toBe('CI · 1:00');
  });

  it('names the workflow when workflows differ, and the branch when branches differ', () => {
    const workflows = [named('CI'), named('Platforms')],
      byWorkflow = ciRunNaming(workflows);
    expect(byWorkflow).toEqual({ workflows: true, branches: false });
    expect(workflows.map((run) => ciRunName(run, byWorkflow))).toEqual(['CI', 'Platforms']);
    expect(ciChipText(workflows[1], ciRunName(workflows[1], byWorkflow))).toBe('Platforms · 1:00');
    expect(ciRunLabel(workflows[1], byWorkflow)).toBe('main Platforms');
    expect(ciRow(workflows[1], byWorkflow).label).toBe('Platforms running');
    expect(ciRow({ ...named('CI', 'docs'), status: 'queued' }, { workflows: false, branches: true }).label).toBe(
      'docs CI queued',
    );

    const branches = [named('CI', 'main'), named('CI', 'docs')],
      byBranch = ciRunNaming(branches);
    expect(byBranch).toEqual({ workflows: false, branches: true });
    expect(branches.map((run) => ciRunName(run, byBranch))).toEqual(['main CI', 'docs CI']);
    expect(ciChipText({ ...branches[1], status: 'queued' }, 'docs CI')).toBe('docs CI · queued');
    expect(ciChipText({ ...branches[1], status: 'failure' }, 'docs CI')).toBe('docs CI failed');

    const both = [named('CI', 'main'), named('Platforms', 'release/2026-11-long-name')];
    expect(ciRunName(both[1], ciRunNaming(both))).toBe('release/2026-11… Platforms');
    // A run without a workflow reads CI among named ones.
    const unnamed = [{ ...ci('queued'), branch: 'main' }, named('Platforms')];
    expect(ciRunName(unnamed[0], ciRunNaming(unnamed))).toBe('CI');
    expect(ciRunName(named('A very long workflow name'), { workflows: true, branches: false })).toBe(
      'A very long wor…',
    );
  });
});

describe('Clear CI Status target (HS2-7PSBHQ, HS2-D3JJ49)', () => {
  const single = { workflows: false, branches: false };
  const run = (status: RightNowCi['status'], named = true): RightNowCi => ({
    status,
    branch: 'main',
    reportedBranch: 'main',
    workflow: 'CI',
    ...(named ? { checkout: 'hotsheet2-56cb', reportedAtMs: 1_795_693_115_028 } : {}),
  });

  it('offers to clear any shown run that names its checkout and report time', () => {
    for (const status of ['in_progress', 'queued', 'failure', 'success', 'cancelled'] as const)
      expect(ciClearTarget(run(status), single), status).toEqual({
        checkout: 'hotsheet2-56cb',
        reportedAtMs: 1_795_693_115_028,
        branch: 'main',
        workflow: 'CI',
        label: 'main CI',
      });
    expect(ciClearTarget(run('unavailable'), single)).toBeUndefined();
    expect(ciClearTarget(run('failure', false), single)).toBeUndefined();
    expect(ciClearTarget({ ...run('failure'), reportedAtMs: undefined }, single)).toBeUndefined();
    // A report without a branch or workflow is named by the time alone.
    const bare: RightNowCi = { status: 'queued', branch: 'main', checkout: 'c', reportedAtMs: 5 };
    expect(ciClearTarget(bare, single)).toEqual({ checkout: 'c', reportedAtMs: 5, label: 'main CI' });
  });

  it('round-trips through data attributes and refuses a missing or malformed time', () => {
    const target = ciClearTarget(run('queued'), { workflows: true, branches: false })!;
    const attributes = ciClearAttributes(target);
    expect(attributes).toEqual({
      'data-ci-clear-checkout': 'hotsheet2-56cb',
      'data-ci-clear-reported-at-ms': '1795693115028',
      'data-ci-clear-branch': 'main',
      'data-ci-clear-workflow': 'CI',
      'data-ci-clear-label': 'main CI',
    });
    const dataset = {
      ciClearCheckout: attributes['data-ci-clear-checkout'],
      ciClearReportedAtMs: attributes['data-ci-clear-reported-at-ms'],
      ciClearBranch: attributes['data-ci-clear-branch'],
      ciClearWorkflow: attributes['data-ci-clear-workflow'],
      ciClearLabel: attributes['data-ci-clear-label'],
    };
    expect(ciClearTargetOf(dataset)).toEqual(target);
    expect(ciClearTargetOf({ ciClearCheckout: 'c-1', ciClearReportedAtMs: '0' })).toEqual({
      checkout: 'c-1',
      reportedAtMs: 0,
      label: 'CI',
    });
    for (const [checkout, at] of [
      [undefined, '1'],
      ['', '1'],
      ['c-1', undefined],
      ['c-1', ''],
      ['c-1', '-1'],
      ['c-1', '1.5'],
      ['c-1', '12abc'],
    ] as const)
      expect(
        ciClearTargetOf({
          ...(checkout === undefined ? {} : { ciClearCheckout: checkout }),
          ...(at === undefined ? {} : { ciClearReportedAtMs: at }),
        }),
        `${checkout} ${at}`,
      ).toBeUndefined();
  });

  it('labels the menu item by the run only when the menu lists several', () => {
    const target = ciClearTarget(run('queued'), { workflows: true, branches: false })!;
    expect(ciClearItemLabel(target, false)).toBe('Clear CI Status');
    expect(ciClearItemLabel({ ...target, label: 'main Platforms' }, true)).toBe('Clear main Platforms Status');
  });
});
