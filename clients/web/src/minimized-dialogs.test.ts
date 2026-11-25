import { Plus, Sparkles } from 'lucide';
import { describe, expect, it } from 'vitest';

import {
  closeAllConfirmation,
  loadMinimizedRecords,
  type MinimizedDialogItem,
  type MinimizedDialogKind,
  minimizedItemsForTab,
  restoreAllPlan,
  saveMinimizedRecords,
} from './minimized-dialogs';

const item = (id: string, kind: string, minimizedAt: number, draft?: string): MinimizedDialogItem => ({
  id,
  kind,
  label: id,
  shortLabel: id,
  icon: kind === 'new-ticket' ? { icon: Plus, name: 'plus' } : { icon: Sparkles, name: 'sparkles' },
  draft,
  minimizedAt,
});

function fakeKind(kind: string, byTab: Record<string, MinimizedDialogItem[]>): MinimizedDialogKind {
  return {
    kind,
    items: (tab) => byTab[tab] ?? [],
    restore: () => Promise.resolve(true),
    discard: () => undefined,
    dispose: () => undefined,
  };
}

describe('minimized dialog items (HS2-3JR0ZH)', () => {
  it('merges every kind of one tab oldest first, keeping kind order on equal timestamps', () => {
    const readers = fakeKind('ticket-reader', {
        a: [item('r1', 'ticket-reader', 10), item('r2', 'ticket-reader', 30)],
        b: [item('rb', 'ticket-reader', 1)],
      }),
      composer = fakeKind('new-ticket', { a: [item('n', 'new-ticket', 20)] }),
      tie = fakeKind('other', { a: [item('t', 'other', 10)] });
    expect(minimizedItemsForTab([readers, composer, tie], 'a').map((entry) => entry.id)).toEqual([
      'r1',
      't',
      'n',
      'r2',
    ]);
    expect(minimizedItemsForTab([readers, composer], 'b').map((entry) => entry.id)).toEqual(['rb']);
    expect(minimizedItemsForTab([readers, composer], 'missing')).toEqual([]);
    expect(minimizedItemsForTab([], 'a')).toEqual([]);
  });

  it('restores each kind in one batch, the kind with the newest item last so it ends on top', () => {
    expect(
      restoreAllPlan([item('r1', 'ticket-reader', 10), item('n', 'new-ticket', 20), item('r2', 'ticket-reader', 30)]),
    ).toEqual([
      { kind: 'new-ticket', ids: ['n'] },
      { kind: 'ticket-reader', ids: ['r1', 'r2'] },
    ]);
    expect(restoreAllPlan([item('r1', 'ticket-reader', 10), item('n', 'new-ticket', 40)])).toEqual([
      { kind: 'ticket-reader', ids: ['r1'] },
      { kind: 'new-ticket', ids: ['n'] },
    ]);
    expect(restoreAllPlan([])).toEqual([]);
  });

  it('asks before Close All only when an item holds a draft, counting the drafted ones', () => {
    expect(closeAllConfirmation([item('a', 'ticket-reader', 1), item('b', 'new-ticket', 2)])).toBeUndefined();
    expect(closeAllConfirmation([])).toBeUndefined();
    expect(closeAllConfirmation([item('a', 'ticket-reader', 1, 'Unsent note'), item('b', 'new-ticket', 2)])).toEqual({
      title: 'Close 2 minimized tickets?',
      message: 'One of them has unsent text that will be discarded.',
      confirmLabel: 'Close All',
      tone: 'danger',
    });
    expect(
      closeAllConfirmation([
        item('a', 'ticket-reader', 1, 'Unsent note'),
        item('b', 'new-ticket', 2, 'Unsaved ticket'),
        item('c', 'ticket-reader', 3),
      ])?.message,
    ).toBe('2 of them have unsent text that will be discarded.');
    expect(closeAllConfirmation([item('b', 'new-ticket', 2, 'Unsaved ticket')])).toMatchObject({
      title: 'Close the minimized ticket?',
      message: 'It has unsent text that will be discarded.',
    });
  });
});

describe('minimized record persistence (HS2-3JR0ZH)', () => {
  const parse = (raw: string | null | undefined) => (raw ? (JSON.parse(raw) as number[]) : []);
  const serialize = (list: readonly number[]) => JSON.stringify(list);

  it('round-trips a list and removes the key once the list is empty', () => {
    const values = new Map<string, string>();
    const storage = {
      getItem: (key: string) => values.get(key) ?? null,
      setItem: (key: string, value: string) => values.set(key, value),
      removeItem: (key: string) => values.delete(key),
    };
    saveMinimizedRecords(storage, 'k', [1, 2], serialize);
    expect(loadMinimizedRecords(storage, 'k', parse)).toEqual([1, 2]);
    saveMinimizedRecords(storage, 'k', [], serialize);
    expect(values.has('k')).toBe(false);
    expect(loadMinimizedRecords(storage, 'k', parse)).toEqual([]);
  });

  it('survives unavailable or throwing storage and malformed values', () => {
    const throwing = {
      getItem: () => {
        throw new Error('denied');
      },
      setItem: () => {
        throw new Error('quota');
      },
      removeItem: () => {
        throw new Error('denied');
      },
    };
    expect(loadMinimizedRecords(throwing, 'k', parse)).toEqual([]);
    expect(() => {
      saveMinimizedRecords(throwing, 'k', [1], serialize);
    }).not.toThrow();
    expect(loadMinimizedRecords(undefined, 'k', parse)).toEqual([]);
    expect(loadMinimizedRecords({ getItem: () => '{' }, 'k', parse)).toEqual([]);
  });
});
