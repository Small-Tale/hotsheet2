import { describe, expect, it } from 'vitest';

import { NoteRepairReview } from './note-repair-review';

const base = {
  noteId: 'synthetic-note',
  before: 'Original synthetic note',
  beforeKind: 'regular',
  response: '{}',
  report: {
    ticket_id: 'synthetic-ticket',
    findings: [{ id: 'kind', note_id: 'synthetic-note', description: 'Wrong kind', proposed_operation: 'Change kind' }],
  },
};

describe('HS2-5E8Y9R repair review', () => {
  it('shows a kind-only proposal and explicit acceptance without repeating unchanged text', () => {
    const markup = String(
      NoteRepairReview({
        ...base,
        preview: {
          proposal: { note_id: base.noteId, kind: 'feedback_needed' },
          patch: { note_kind: 'feedback_needed' },
          changed: true,
          unavailable: [],
        },
      }),
    );
    expect(markup).toContain('Type: regular');
    expect(markup).toContain('feedback_needed');
    expect(markup).toContain('Text unchanged');
    expect(markup).toContain('Apply Repair');
    expect(markup).not.toContain('Original synthetic note');
  });

  it('explains unsupported operations and disables acceptance', () => {
    const markup = String(
      NoteRepairReview({
        ...base,
        preview: {
          proposal: { note_id: base.noteId, summary: 'New summary' },
          patch: {},
          changed: false,
          unavailable: ['Summary edits are unavailable'],
        },
      }),
    );
    expect(markup).toContain('Summary edits are unavailable');
    expect(markup).toMatch(/data-action="apply-note-repair"[^>]*disabled/u);
  });

  it('HS2-3TNEJF keys each Cannot Apply banner by its reason so Kerf diffs the list by identity', () => {
    const markup = String(
      NoteRepairReview({
        ...base,
        preview: {
          proposal: { note_id: base.noteId, summary: 'New summary' },
          patch: {},
          changed: false,
          unavailable: ['This source cannot edit notes.', 'Editing an existing note summary is not supported yet.'],
        },
      }),
    );
    const keys = [...markup.matchAll(/<[^>]*\bkui-state-banner\b[^>]*>/gu)].map(
      ([tag]) => /\bdata-key="([^"]*)"/u.exec(tag)?.[1],
    );
    expect(keys).toEqual([
      'cannot-apply:This source cannot edit notes.',
      'cannot-apply:Editing an existing note summary is not supported yet.',
    ]);
  });
});
