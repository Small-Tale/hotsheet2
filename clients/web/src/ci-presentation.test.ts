import { describe, expect, it } from 'vitest';

import type { LiveCiStatus } from './api';
import {
  CI_PRESENTATION,
  ciAge,
  ciChipText,
  ciChipTicks,
  ciClearTarget,
  ciClearTargetOf,
  ciDetail,
  ciRow,
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

describe('stale CI report words (HS2-CC7TCB)', () => {
  const stale = (lastSeen: 'running' | 'queued', ageMs?: number): RightNowCi => ({
    status: 'unknown',
    lastSeen,
    branch: 'main',
    shortSha: '8f6c8d8',
    ...(ageMs === undefined ? {} : { ageMs }),
  });

  it('names the chip without a clock and never ticks', () => {
    expect(ciChipText(stale('running', 2 * HOUR + 39 * MINUTE))).toBe('CI · unknown');
    expect(ciChipText(stale('queued'))).toBe('CI · unknown');
    expect(ciChipTicks(stale('running', 2 * HOUR))).toBe(false);
  });

  it('says when the run was last seen and what it was doing', () => {
    expect(ciAge(stale('running', 2 * HOUR + 39 * MINUTE))).toBe('last seen running 2h 39m ago');
    expect(ciAge(stale('queued', 3 * HOUR))).toBe('last seen queued 3h 00m ago');
    expect(ciAge(stale('running'))).toBeUndefined();
    expect(ciDetail(stale('running', 2 * HOUR + 39 * MINUTE))).toBe('main CI · 8f6c8d8 · last seen running 2h 39m ago');
    expect(CI_PRESENTATION.unknown).toMatchObject({ word: 'Unknown', name: 'circle-help' });
  });

  it('fits the expanded sidebar row', () => {
    expect(ciRow(stale('running', 2 * HOUR + 10 * MINUTE))).toEqual({
      label: 'main CI unknown',
      trailing: 'seen 2h 10m ago',
    });
    expect(ciRow({ status: 'unknown', lastSeen: 'queued', branch: 'main' })).toEqual({
      label: 'main CI unknown',
      trailing: '',
    });
    expect(ciRow(ci('success', 12 * MINUTE))).toEqual({ label: 'main CI passed · 9f8e7d6', trailing: '12m ago' });
    expect(ciRow(ci('in_progress', 4 * MINUTE))).toEqual({
      label: 'main CI running · 9f8e7d6',
      trailing: 'running 4m',
    });
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

describe('Clear CI Status target (HS2-7PSBHQ)', () => {
  const run = (status: RightNowCi['status'], named = true): RightNowCi => ({
    status,
    branch: 'main',
    ...(named ? { checkout: 'hotsheet2-56cb', reportedAtMs: 1_795_693_115_028 } : {}),
  });

  it('offers to clear only a run shown as unknown that names its checkout and report time', () => {
    expect(ciClearTarget(run('unknown'))).toEqual({ checkout: 'hotsheet2-56cb', reportedAtMs: 1_795_693_115_028 });
    for (const status of ['in_progress', 'queued', 'failure', 'success', 'cancelled', 'unavailable'] as const)
      expect(ciClearTarget(run(status)), status).toBeUndefined();
    expect(ciClearTarget(run('unknown', false))).toBeUndefined();
    expect(ciClearTarget({ ...run('unknown'), reportedAtMs: undefined })).toBeUndefined();
  });

  it('reads the target back from data attributes, refusing a missing or malformed time', () => {
    expect(ciClearTargetOf('c-1', '1795693115028')).toEqual({ checkout: 'c-1', reportedAtMs: 1_795_693_115_028 });
    expect(ciClearTargetOf('c-1', '0')).toEqual({ checkout: 'c-1', reportedAtMs: 0 });
    for (const [checkout, at] of [
      [undefined, '1'],
      ['', '1'],
      ['c-1', undefined],
      ['c-1', ''],
      ['c-1', '-1'],
      ['c-1', '1.5'],
      ['c-1', '12abc'],
    ] as const)
      expect(ciClearTargetOf(checkout, at), `${checkout} ${at}`).toBeUndefined();
  });
});
