import { describe, expect, it } from 'vitest';

import type { Capabilities, Note } from './api';
import { responseLinkLabel, responseLinkTargets, responseLinkUnavailable } from './note-response-link';

const note = (
  id: string,
  kind: Note['kind'],
  created_at: string,
  text = 'Synthetic',
  extra: Partial<Note> = {},
): Note => ({
  id,
  kind,
  created_at,
  edited_at: created_at,
  text,
  ...extra,
});
const first = note('01A', 'feedback_needed', '2026-11-01T00:00:00Z', 'FEEDBACK NEEDED: first question');
const second = note('01B', 'feedback_needed', '2026-11-01T01:00:00.500000Z', '> **FEEDBACK NEEDED:** second');
const decision = note('01C', 'regular', '2026-11-01T02:00:00Z', 'Decision: option A');
const later = note('01D', 'feedback_needed', '2026-11-01T03:00:00Z');
const capable = { note_edit: true, note_response_link: true } as Capabilities;

describe('HS2-J4G9GB note response links', () => {
  it('offers only earlier presented requests, ordered as stored', () => {
    expect(responseLinkTargets([first, second, decision, later], decision.id).map((item) => item.id)).toEqual([
      '01A',
      '01B',
    ]);
    // Equal times fall back to id order, like the core.
    const tie = note('01E', 'regular', later.created_at);
    expect(responseLinkTargets([later, tie], tie.id)).toEqual([later]);
    expect(responseLinkTargets([first], 'missing')).toEqual([]);
    expect(responseLinkTargets([first], first.id)).toEqual([]);
  });

  it('explains every unavailable state and allows relinking a linked note', () => {
    const notes = [first, second, decision];
    expect(responseLinkUnavailable(notes, decision.id, capable)).toBeUndefined();
    expect(responseLinkUnavailable(notes, decision.id, undefined)).toContain('cannot link');
    expect(responseLinkUnavailable(notes, decision.id, { note_edit: true })).toContain('cannot link');
    expect(responseLinkUnavailable(notes, decision.id, capable, 'activity')).toBe(
      'Only a regular note can answer a feedback request.',
    );
    expect(responseLinkUnavailable(notes, first.id, capable)).toBe(
      'Only a regular note can answer a feedback request.',
    );
    const rating = note('01F', 'regular', later.created_at, 'Rated', {
      ai_feedback: { target: 'note:01C', rating: 'helpful' },
    });
    expect(responseLinkUnavailable([first, rating], rating.id, capable)).toBe(
      'Only a regular note can answer a feedback request.',
    );
    const lonely = note('01G', 'regular', first.created_at);
    expect(responseLinkUnavailable([lonely, later], lonely.id, capable)).toBe(
      'No earlier feedback request on this ticket.',
    );
    // An orphaned link can still be cleared even with no remaining target.
    expect(responseLinkUnavailable([{ ...lonely, response_to: 'gone' }], lonely.id, capable)).toBeUndefined();
  });

  it('labels a request by its first meaningful line', () => {
    expect(responseLinkLabel(first)).toBe('first question');
    expect(responseLinkLabel(second)).toBe('second');
    expect(responseLinkLabel({ text: 'FEEDBACK NEEDED' })).toBe('Feedback request');
    expect(responseLinkLabel({ text: `FEEDBACK NEEDED: ${'x'.repeat(100)}` }, 10)).toBe(`${'x'.repeat(9)}…`);
  });
});
