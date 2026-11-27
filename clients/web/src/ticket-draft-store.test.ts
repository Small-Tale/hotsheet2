import { describe, expect, it } from 'vitest';

import {
  captureTicketDraft,
  clearTicketDraft,
  loadRestorableTicketDraft,
  loadTicketDraft,
  planTicketDraftRestore,
  prepareCommittedDraftCleanup,
  reconcileStoredTicketDraft,
  saveTicketDraft,
  ticketDraftKey,
} from './ticket-draft-store';

function memoryStorage(): Storage & { map: Map<string, string> } {
  const map = new Map<string, string>();
  return {
    map,
    get length() {
      return map.size;
    },
    clear: () => {
      map.clear();
    },
    getItem: (key) => map.get(key) ?? null,
    key: (index) => [...map.keys()][index] ?? null,
    removeItem: (key) => void map.delete(key),
    setItem: (key, value) => void map.set(key, value),
  };
}

describe('ticket draft store (HS2-RE1PS6)', () => {
  it('HS2-E0H7S0 clears only the captured owner with field-appropriate equality after a save', () => {
    const storage = memoryStorage();
    for (const field of [
      'details',
      'note',
      'title',
      'blocked_reason',
      'attachment_label',
      'attachment_filename',
    ] as const) {
      const a = ticketDraftKey('project-a', 'connection:1', field),
        b = ticketDraftKey('project-b', 'connection:1', field);
      saveTicketDraft(storage, a, { base: 'base-a', draft: 'submitted' });
      const cleanup = prepareCommittedDraftCleanup(storage, a, field, 'submitted');
      saveTicketDraft(storage, a, { base: 'base-a', draft: 'submitted\n  ' });
      saveTicketDraft(storage, b, { base: 'base-b', draft: 'submitted' });
      cleanup();
      expect(loadTicketDraft(storage, b)?.draft).toBe('submitted');
      expect(loadTicketDraft(storage, a)?.draft).toBe(
        field === 'details' || field === 'note' ? 'submitted\n  ' : undefined,
      );
      saveTicketDraft(storage, a, { base: 'base-a', draft: 'different later edit' });
      cleanup();
      expect(loadTicketDraft(storage, a)?.draft).toBe('different later edit');
      saveTicketDraft(storage, a, { base: 'base-a', draft: 'submitted' });
      cleanup();
      expect(loadTicketDraft(storage, a)).toBeUndefined();
    }
    expect(() => {
      prepareCommittedDraftCleanup(undefined, undefined, 'details', '')();
    }).not.toThrow();
  });
  it('HS2-E0H7S0 validates native intent and rebases only over its exact older submitted value', () => {
    const storage = memoryStorage(),
      key = 'hotsheet.ticket-draft:owned';
    for (const draft of ['base', 'latest']) {
      captureTicketDraft(storage, key, { base: 'base', draft, pendingIntent: { predecessor: 'older' } }, 42);
      const stored = loadTicketDraft(storage, key)!;
      expect(stored).toMatchObject({ base: 'base', draft, pendingIntent: { predecessor: 'older' } });
      expect(reconcileStoredTicketDraft(stored, 'older')).toEqual({ kind: 'unchanged', base: 'older', draft });
      expect(reconcileStoredTicketDraft(stored, draft).kind).toBe('converged');
      expect(reconcileStoredTicketDraft(stored, 'unrelated remote').kind).toBe('conflict');
      saveTicketDraft(storage, key, { base: 'base', draft }); // repeated teardown pagehide
      expect(loadTicketDraft(storage, key)?.pendingIntent).toEqual({ predecessor: 'older' });
    }
    for (const pendingIntent of [null, true, {}, { predecessor: 1 }, { predecessor: 'older', extra: true }]) {
      storage.setItem(key, JSON.stringify({ base: 'base', draft: 'latest', pendingIntent }));
      expect(loadTicketDraft(storage, key)).toBeUndefined();
    }
    storage.setItem(key, JSON.stringify({ base: 'base', draft: 'latest', at: 42 }));
    expect(loadTicketDraft(storage, key)).toEqual({ base: 'base', draft: 'latest', at: 42 });
    saveTicketDraft(storage, key, { base: 'base', draft: 'base' });
    expect(loadTicketDraft(storage, key)).toBeUndefined(); // unchanged ordinary legacy behavior
  });
  it('HS2-E0H7S0 exposes unavailable/quota capture while ordinary browser writes stay tolerant', () => {
    const storage = memoryStorage(),
      key = ticketDraftKey('p', 'git:1', 'details'),
      failure = {
        getItem: storage.getItem.bind(storage),
        removeItem: storage.removeItem.bind(storage),
        setItem: () => {
          throw new Error('Synthetic quota');
        },
      };
    expect(() => {
      captureTicketDraft(undefined, key, { base: 'base', draft: 'new' });
    }).toThrow('unavailable');
    expect(() => {
      captureTicketDraft(storage, undefined, { base: 'base', draft: 'new' });
    }).toThrow('unavailable');
    expect(() => {
      captureTicketDraft(failure, key, { base: 'base', draft: 'new' });
    }).toThrow('Synthetic quota');
    expect(() => {
      saveTicketDraft(failure, key, { base: 'base', draft: 'new' });
    }).not.toThrow();
    captureTicketDraft(storage, key, { base: 'base', draft: 'new' }, 42);
    expect(loadTicketDraft(storage, key)).toEqual({ base: 'base', draft: 'new', at: 42 });
    captureTicketDraft(storage, key, { base: 'base', draft: 'base' });
    expect(loadTicketDraft(storage, key)).toBeUndefined();
  });
  it('keys drafts by project, ticket, field, and note', () => {
    expect(ticketDraftKey('p1', 'git:7', 'details')).toBe('hotsheet.ticket-draft:p1:git:7:details');
    expect(ticketDraftKey('p1', 'git:7', 'note', 'n9')).toBe('hotsheet.ticket-draft:p1:git:7:note:n9');
  });

  it('stores the edit-start base beside the draft and drops a draft equal to its base', () => {
    const storage = memoryStorage(),
      key = ticketDraftKey('p', 'git:1', 'details');
    saveTicketDraft(storage, key, { base: 'start', draft: 'start typed' }, 42);
    expect(loadTicketDraft(storage, key)).toEqual({ base: 'start', draft: 'start typed', at: 42 });
    saveTicketDraft(storage, key, { base: 'start', draft: 'start' });
    expect(loadTicketDraft(storage, key)).toBeUndefined();
    saveTicketDraft(storage, key, { base: 'start', draft: 'again' }, 7);
    clearTicketDraft(storage, key);
    expect(storage.map.size).toBe(0);
  });

  it('ignores malformed or missing entries and storage failures', () => {
    const storage = memoryStorage(),
      key = ticketDraftKey('p', 'git:1', 'title');
    storage.setItem(key, '{not json');
    expect(loadTicketDraft(storage, key)).toBeUndefined();
    storage.setItem(key, JSON.stringify({ base: 1, draft: 'x' }));
    expect(loadTicketDraft(storage, key)).toBeUndefined();
    const failing = {
      getItem: () => {
        throw new Error('blocked');
      },
      setItem: () => {
        throw new Error('blocked');
      },
      removeItem: () => {
        throw new Error('blocked');
      },
    };
    expect(() => {
      saveTicketDraft(failing, key, { base: 'a', draft: 'b' });
    }).not.toThrow();
    expect(loadTicketDraft(failing, key)).toBeUndefined();
    expect(() => {
      clearTicketDraft(failing, key);
    }).not.toThrow();
  });
});

describe('planTicketDraftRestore (HS2-7D3MPP)', () => {
  const stored = (base: string, draft: string, predecessor?: string) => ({
    base,
    draft,
    at: 1,
    ...(predecessor === undefined ? {} : { pendingIntent: { predecessor } }),
  });

  it('restores nothing without a copy, and clears a copy that is redundant', () => {
    expect(planTicketDraftRestore(undefined, 'Intro')).toEqual({ kind: 'none', clear: false });
    // The draft never left its base, or the server already holds it.
    expect(planTicketDraftRestore(stored('Intro', 'Intro'), 'Intro changed')).toEqual({ kind: 'none', clear: true });
    expect(planTicketDraftRestore(stored('Intro', 'Intro mine'), 'Intro mine')).toEqual({ kind: 'none', clear: true });
  });

  it('continues an unchanged base and merges a disjoint server change', () => {
    expect(planTicketDraftRestore(stored('Intro', 'Intro mine'), 'Intro')).toEqual({
      kind: 'restore',
      base: 'Intro',
      draft: 'Intro mine',
      merged: false,
    });
    expect(planTicketDraftRestore(stored('A\n\nB', 'A mine\n\nB'), 'A\n\nB theirs')).toEqual({
      kind: 'restore',
      base: 'A\n\nB theirs',
      draft: 'A mine\n\nB theirs',
      merged: true,
    });
  });

  it('offers both versions when the server changed the same text, instead of discarding the draft', () => {
    expect(planTicketDraftRestore(stored('Intro', 'Intro rewritten here'), 'Intro replaced elsewhere')).toEqual({
      kind: 'conflict',
      base: 'Intro',
      mine: 'Intro rewritten here',
      theirs: 'Intro replaced elsewhere',
    });
    // Native revert intent over a value that moved on is a conflict too, on the copy's own base.
    expect(planTicketDraftRestore(stored('old', 'old', 'submitted'), 'remote')).toEqual({
      kind: 'conflict',
      base: 'old',
      mine: 'old',
      theirs: 'remote',
    });
    // The same intent over its exact predecessor still restores.
    expect(planTicketDraftRestore(stored('old', 'old', 'submitted'), 'submitted')).toEqual({
      kind: 'restore',
      base: 'submitted',
      draft: 'old',
      merged: false,
    });
  });
});

describe('planTicketDraftRestore auto-resolution (HS2-JJ3R1G)', () => {
  const stored = (base: string, draft: string) => ({ base, draft, at: 1 });

  it('merges disjoint edits silently into the editor and says the restore was merged', () => {
    // Different lines, then edits at the start and end of one line, then a whitespace-only edit kept exactly.
    expect(planTicketDraftRestore(stored('a\nb\nc', 'A\nb\nc'), 'a\nb\nC')).toEqual({
      kind: 'restore',
      base: 'a\nb\nC',
      draft: 'A\nb\nC',
      merged: true,
    });
    expect(planTicketDraftRestore(stored('middle', 'start middle'), 'middle end')).toEqual({
      kind: 'restore',
      base: 'middle end',
      draft: 'start middle end',
      merged: true,
    });
    expect(planTicketDraftRestore(stored('a\nb', 'a  \nb'), 'a\nb theirs')).toEqual({
      kind: 'restore',
      base: 'a\nb theirs',
      draft: 'a  \nb theirs',
      merged: true,
    });
  });

  it('restores an empty-base draft when the server stayed empty, and asks when both typed', () => {
    expect(planTicketDraftRestore(stored('', 'typed'), '')).toEqual({
      kind: 'restore',
      base: '',
      draft: 'typed',
      merged: false,
    });
    expect(planTicketDraftRestore(stored('', 'mine'), 'theirs')).toEqual({
      kind: 'conflict',
      base: '',
      mine: 'mine',
      theirs: 'theirs',
    });
  });

  it('clears the copy when the server already holds the merge (a reload during the merged save)', () => {
    // The save merged `mine` with a disjoint change and committed; the page reloaded before the copy cleared.
    expect(planTicketDraftRestore(stored('a\nb', 'A\nb'), 'A\nB')).toEqual({ kind: 'none', clear: true });
    // A server that took my edit and then moved on elsewhere already holds my text.
    expect(planTicketDraftRestore(stored('a\nb', 'A\nb'), 'A\nb\nc')).toEqual({ kind: 'none', clear: true });
  });

  it('asks again for a repeated conflict and keeps both texts each time', () => {
    const copy = stored('Intro', 'Intro mine');
    for (const theirs of ['Intro theirs', 'Intro theirs again'])
      expect(planTicketDraftRestore(copy, theirs)).toEqual({
        kind: 'conflict',
        base: 'Intro',
        mine: 'Intro mine',
        theirs,
      });
  });

  it('loads a copy without a merge base and falls back to the conflict card, never dropping it', () => {
    const storage = memoryStorage(),
      key = ticketDraftKey('p', 'git-local:1', 'details');
    storage.setItem(key, JSON.stringify({ draft: 'Legacy text', at: 1 }));
    expect(loadTicketDraft(storage, key)).toBeUndefined();
    const legacy = loadRestorableTicketDraft(storage, key);
    expect(legacy).toEqual({ legacy: true, draft: 'Legacy text' });
    expect(planTicketDraftRestore(legacy, 'Server text')).toEqual({
      kind: 'conflict',
      base: '',
      mine: 'Legacy text',
      theirs: 'Server text',
    });
    expect(planTicketDraftRestore(legacy, 'Legacy text')).toEqual({ kind: 'none', clear: true });
    // A current copy still loads normally; malformed shapes are ignored.
    storage.setItem(key, JSON.stringify({ base: 'b', draft: 'd', at: 2 }));
    expect(loadRestorableTicketDraft(storage, key)).toEqual({ base: 'b', draft: 'd', at: 2 });
    for (const raw of ['{', '[]', JSON.stringify({ base: 3, draft: 'd' }), JSON.stringify({ draft: 4 })]) {
      storage.setItem(key, raw);
      expect(loadRestorableTicketDraft(storage, key), raw).toBeUndefined();
    }
    expect(loadRestorableTicketDraft(storage, 'missing')).toBeUndefined();
  });
});
