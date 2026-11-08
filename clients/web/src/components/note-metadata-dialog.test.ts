import { describe, expect, it } from 'vitest';

import { NoteMetadataDialog, type NoteMetadataDialogState } from './note-metadata-dialog';

const state: NoteMetadataDialogState = {
  noteId: 'synthetic-note',
  ticketId: 'synthetic:ticket',
  projectId: 'synthetic-project',
  token: 'synthetic-token',
  mode: 'kind',
  kind: 'feedback_needed',
  hasResponses: false,
};

describe('NoteMetadataDialog', () => {
  it('HS2-Q0GWXT confirms a kind change that closes an unanswered request', () => {
    const markup = String(NoteMetadataDialog({ state }));
    expect(markup).toContain('Change note type');
    expect(markup).toContain('Changing this type closes the unanswered feedback request.');
    expect(markup).toContain('Confirm type change');
  });

  it('HS2-Q0GWXT offers separate clearable summary and confidence fields', () => {
    const summary = String(
      NoteMetadataDialog({ state: { ...state, mode: 'summary', kind: 'activity', summary: 'First' } }),
    );
    expect(summary).toContain('name="note-metadata-summary"');
    expect(summary).toContain('Leave this blank to clear the summary.');
    const confidence = String(NoteMetadataDialog({ state: { ...state, mode: 'confidence', confidence: 80 } }));
    expect(confidence).toContain('name="note-metadata-confidence"');
    expect(confidence).toContain('Enter 0–100, or leave blank to clear the score.');
  });
});
