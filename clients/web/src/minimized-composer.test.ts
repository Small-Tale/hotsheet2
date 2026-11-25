import { describe, expect, it } from 'vitest';

import {
  addMinimizedComposer,
  composerHasDraft,
  loadMinimizedComposers,
  MINIMIZED_COMPOSERS_STORAGE_KEY,
  type MinimizedComposer,
  minimizedComposerDraftLabel,
  minimizedComposerId,
  minimizedComposerLabel,
  parseMinimizedComposers,
  saveMinimizedComposers,
  serializeMinimizedComposers,
  withoutMinimizedComposers,
} from './minimized-composer';

const entry = (tabProjectId: string, overrides: Partial<MinimizedComposer> = {}): MinimizedComposer => ({
  id: minimizedComposerId(tabProjectId),
  tabProjectId,
  title: '',
  hasDraft: false,
  minimizedAt: 1,
  ...overrides,
});

describe('minimized composer records (HS2-3JR0ZH)', () => {
  it('names the composer by its title, or Untitled', () => {
    expect(minimizedComposerLabel('  Fix the release notes ')).toBe('New ticket: Fix the release notes');
    expect(minimizedComposerLabel('   ')).toBe('New ticket: Untitled');
    expect(minimizedComposerId('alpha')).toBe('new-ticket:alpha');
  });

  it('counts a title, details, or a staged file as a draft, but not whitespace or metadata alone', () => {
    expect(composerHasDraft({ title: '', details: '', attachmentCount: 0 })).toBe(false);
    expect(composerHasDraft({ title: '  ', details: '\n', attachmentCount: 0 })).toBe(false);
    expect(composerHasDraft({ title: 'T', details: '', attachmentCount: 0 })).toBe(true);
    expect(composerHasDraft({ title: '', details: 'D', attachmentCount: 0 })).toBe(true);
    expect(composerHasDraft({ title: '', details: '', attachmentCount: 1 })).toBe(true);
    expect(minimizedComposerDraftLabel(entry('a', { hasDraft: true }))).toBe('Unsaved ticket');
    expect(minimizedComposerDraftLabel(entry('a'))).toBeUndefined();
  });

  it('keeps one record per tab, replacing an earlier one, and drops closed tabs', () => {
    const first = entry('a', { minimizedAt: 1 }),
      other = entry('b', { minimizedAt: 2 }),
      again = entry('a', { title: 'Later', minimizedAt: 3 });
    const list = addMinimizedComposer(addMinimizedComposer(addMinimizedComposer([], first), other), again);
    expect(list).toEqual([other, again]);
    expect(withoutMinimizedComposers(list, new Set(['a']))).toEqual([other]);
    expect(withoutMinimizedComposers(list, new Set())).toEqual(list);
  });

  it('round-trips through storage and drops malformed records and duplicate tabs', () => {
    const list = [entry('a', { title: 'Draft', hasDraft: true, minimizedAt: 5 }), entry('b')];
    expect(parseMinimizedComposers(serializeMinimizedComposers(list))).toEqual(list);
    expect(
      parseMinimizedComposers(
        JSON.stringify([
          { tabProjectId: 'a', title: 'First', hasDraft: true, minimizedAt: 2, id: 'spoofed' },
          { tabProjectId: 'a', title: 'Second' },
          { tabProjectId: '' },
          { title: 'No tab' },
          'junk',
          { tabProjectId: 'c', title: 7, hasDraft: 'yes', minimizedAt: 'soon' },
        ]),
      ),
    ).toEqual([entry('a', { title: 'First', hasDraft: true, minimizedAt: 2 }), entry('c', { minimizedAt: 0 })]);
    expect(parseMinimizedComposers('{')).toEqual([]);
    expect(parseMinimizedComposers('{"a":1}')).toEqual([]);
    expect(parseMinimizedComposers(null)).toEqual([]);

    const values = new Map<string, string>();
    const storage = {
      getItem: (key: string) => values.get(key) ?? null,
      setItem: (key: string, value: string) => values.set(key, value),
      removeItem: (key: string) => values.delete(key),
    };
    saveMinimizedComposers(storage, list);
    expect(values.has(MINIMIZED_COMPOSERS_STORAGE_KEY)).toBe(true);
    expect(loadMinimizedComposers(storage)).toEqual(list);
    saveMinimizedComposers(storage, []);
    expect(values.has(MINIMIZED_COMPOSERS_STORAGE_KEY)).toBe(false);
  });
});
