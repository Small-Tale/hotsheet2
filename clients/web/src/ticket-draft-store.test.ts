import { describe, expect, it } from 'vitest';

import {
  captureTicketDraft,
  clearTicketDraft,
  loadTicketDraft,
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
