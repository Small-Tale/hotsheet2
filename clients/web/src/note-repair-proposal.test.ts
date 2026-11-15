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
type NoteRepairProposalInput = [target: string, kind?: string];

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
      'This source cannot link an existing note as a feedback response.',
    ]);
    expect(preview.patch).not.toHaveProperty('response_to');
    expect(preview.patch).not.toHaveProperty('note_summary');
  });

  describe('HS2-J4G9GB existing-note response links', () => {
    const request: Note = {
      id: '01SYNTHETICREQUEST0000000000',
      kind: 'feedback_needed',
      feedback_request: true,
      text: 'FEEDBACK NEEDED: pick one',
      created_at: '2025-12-31T00:00:00Z',
      edited_at: '2025-12-31T00:00:00Z',
    };
    const other: Note = { ...request, id: '01SYNTHETICREQUEST0000000001', created_at: '2025-12-31T01:00:00Z' };
    const later: Note = { ...request, id: '01SYNTHETICREQUEST0000000002', created_at: '2026-02-01T00:00:00Z' };
    const decision: Note = { ...note, text: 'Synthetic decision: option A' };
    const linkable = { ...capabilities, note_response_link: true } as Capabilities;
    const propose = (response_to: string | null, kind?: string) =>
      parseNoteRepairProposal(JSON.stringify({ note_id: note.id, response_to, ...(kind ? { kind } : {}) }), note.id);

    it('links, relinks, and unlinks the existing note through one patch', () => {
      const notes = [request, other, decision];
      const linked = previewNoteRepair(propose(request.id), decision, linkable, 'synthetic-token', notes);
      expect(linked).toMatchObject({
        unavailable: [],
        changed: true,
        patch: { note_id: note.id, response_to: request.id, expected_token: 'synthetic-token' },
      });
      expect(linked.patch).not.toHaveProperty('note');
      const relinked = previewNoteRepair(propose(other.id), { ...decision, response_to: request.id }, linkable, '', [
        request,
        other,
        { ...decision, response_to: request.id },
      ]);
      expect(relinked.patch).toMatchObject({ response_to: other.id });
      const unlinked = previewNoteRepair(propose(null), { ...decision, response_to: request.id }, linkable, '', [
        request,
        { ...decision, response_to: request.id },
      ]);
      expect(unlinked.patch).toMatchObject({ response_to: null });
      expect(unlinked.changed).toBe(true);
    });

    it('treats an already linked note as a no-op', () => {
      const same = previewNoteRepair(propose(request.id), { ...decision, response_to: request.id }, linkable, '', [
        request,
        { ...decision, response_to: request.id },
      ]);
      expect(same).toMatchObject({ unavailable: [], changed: false });
      expect(same.patch).not.toHaveProperty('response_to');
    });

    it('names each reason a link cannot apply and never patches it', () => {
      const cases: Array<[NoteRepairProposalInput, Note[], string]> = [
        [[later.id], [request, decision, later], 'does not name an earlier feedback request'],
        [['01UNKNOWN000000000000000000'], [request, decision], 'does not name an earlier feedback request'],
        [[request.id, 'activity'], [request, decision], 'Only a regular note can answer a feedback request.'],
        [[request.id], [decision], 'No earlier feedback request on this ticket.'],
      ];
      for (const [[target, kind], notes, reason] of cases) {
        const preview = previewNoteRepair(propose(target, kind), decision, linkable, '', notes);
        expect(preview.unavailable.join(' ')).toContain(reason);
        expect(preview.patch).not.toHaveProperty('response_to');
      }
    });
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
