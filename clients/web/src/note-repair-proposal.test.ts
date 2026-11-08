import { describe, expect, it } from 'vitest';

import type { Capabilities, Note } from './api';
import { parseNoteRepairProposal, previewNoteRepair } from './note-repair-proposal';

const note: Note = {
  id: '01SYNTHETICNOTE0000000000000',
  kind: 'regular',
  kind_explicit: true,
  text: 'FEEDBACK NEEDED: choose a synthetic option',
  created_at: '2026-01-01T00:00:00Z',
  edited_at: '2026-01-01T00:00:00Z',
  actor: { role: 'ai' },
};
const capabilities = { note_edit: true, note_kind_edit: true } as Capabilities;

describe('HS2-5E8Y9R note repair proposals', () => {
  it('accepts a focused fenced proposal and prepares a human-reviewed kind correction', () => {
    const proposal = parseNoteRepairProposal(
      `\`\`\`json\n{"note_id":"${note.id}","kind":"feedback_needed","explanation":"Correct the note structure."}\n\`\`\``,
      note.id,
    );
    const preview = previewNoteRepair(proposal, note, capabilities, 'synthetic-token');
    expect(preview).toMatchObject({
      unavailable: [],
      changed: true,
      patch: {
        note_id: note.id,
        note_kind: 'feedback_needed',
        expected_token: 'synthetic-token',
        actor: { role: 'human' },
      },
    });
    expect(preview.patch).not.toHaveProperty('response_to');
  });

  it('refuses ticket-wide edits, mismatched notes, draft kind, and invalid confidence', () => {
    for (const source of [
      { note_id: note.id, details: 'rewrite the ticket', text: 'safe' },
      { note_id: 'different', kind: 'activity' },
      { note_id: note.id, kind: 'feedback_draft' },
      { note_id: note.id, confidence: 101 },
      { note_id: note.id },
    ])
      expect(() => parseNoteRepairProposal(JSON.stringify(source), note.id)).toThrow();
  });

  it('shows unavailable operations and cannot turn an existing decision into a reply', () => {
    const proposal = parseNoteRepairProposal(
      JSON.stringify({ note_id: note.id, kind: 'feedback_needed', summary: 'Prompt', response_to: 'request-id' }),
      note.id,
    );
    const preview = previewNoteRepair(proposal, note, { note_edit: true, note_kind_edit: false } as Capabilities);
    expect(preview.unavailable).toEqual([
      'This source cannot change note type.',
      'Editing an existing note summary is not supported yet.',
      'Linking an existing note as a response is not supported yet.',
    ]);
    expect(preview.patch).not.toHaveProperty('response_to');
    expect(preview.patch).not.toHaveProperty('note_summary');
  });

  it('marks an AI prompt text edit explicit and keeps a no-op proposal from applying', () => {
    const prompt = { ...note, kind: 'feedback_needed' as const, feedback_request: true };
    const text = previewNoteRepair(
      parseNoteRepairProposal(JSON.stringify({ note_id: note.id, text: 'FEEDBACK NEEDED: repaired prompt' }), note.id),
      prompt,
      capabilities,
    );
    expect(text.patch).toMatchObject({ note: 'FEEDBACK NEEDED: repaired prompt', edit_feedback_prompt: true });
    const unchanged = previewNoteRepair(
      parseNoteRepairProposal(JSON.stringify({ note_id: note.id, text: note.text }), note.id),
      note,
      capabilities,
    );
    expect(unchanged.changed).toBe(false);
  });

  it('refuses a kind change after a response has already been linked', () => {
    const preview = previewNoteRepair(
      parseNoteRepairProposal(JSON.stringify({ note_id: note.id, kind: 'feedback_needed' }), note.id),
      note,
      capabilities,
      undefined,
      [{ ...note, id: 'response-id', response_to: note.id }],
    );
    expect(preview.unavailable).toContain(
      'This note already has a linked response; changing its type needs separate review.',
    );
  });
});
