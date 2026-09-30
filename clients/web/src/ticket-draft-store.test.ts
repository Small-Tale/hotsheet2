import { describe, expect, it } from 'vitest';

import { clearTicketDraft, loadTicketDraft, saveTicketDraft, ticketDraftKey } from './ticket-draft-store';

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
