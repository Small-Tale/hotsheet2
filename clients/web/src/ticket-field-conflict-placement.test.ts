import { describe, expect, it } from 'vitest';

import { ticketFieldConflictNoteId, ticketFieldConflictPlacement } from './ticket-field-conflict-placement';

describe('HS2-3TSEAX ticket field conflict placement', () => {
  it.each([
    ['details', 'details'],
    ['blocked_reason', 'blocked'],
    ['tags', 'tags'],
    ['category', 'metadata'],
    ['priority', 'metadata'],
    ['status', 'metadata'],
    ['note:01ABC', 'note'],
    ['title', 'header'],
    ['up_next', 'header'],
    ['attachment_label', 'header'],
    ['something_new', 'header'],
  ])('places a %s conflict in %s while Info is shown', (key, placement) => {
    expect(ticketFieldConflictPlacement(key, { infoShown: true })).toBe(placement);
  });

  it.each(['details', 'blocked_reason', 'tags', 'status', 'note:01ABC', 'title'])(
    'keeps a %s conflict in the header while Info is not shown',
    (key) => {
      expect(ticketFieldConflictPlacement(key, { infoShown: false })).toBe('header');
    },
  );

  it('reads the note id only from a note key', () => {
    expect(ticketFieldConflictNoteId('note:01ABC')).toBe('01ABC');
    expect(ticketFieldConflictNoteId('note:')).toBe('');
    expect(ticketFieldConflictNoteId('details')).toBeUndefined();
  });
});
