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

  it('HS2-J4G9GB picks an earlier request or none for an existing note', () => {
    const response = String(
      NoteMetadataDialog({
        state: {
          ...state,
          mode: 'response',
          kind: 'regular',
          responseTo: 'request-b',
          responseTargets: [
            { value: 'request-a', label: 'First question' },
            { value: 'request-b', label: 'Second question' },
          ],
        },
      }),
    );
    expect(response).toContain('Link as feedback response');
    expect(response).toContain('name="note-metadata-response"');
    expect(response).toContain('Not a Response');
    expect(response).toContain('First question');
    expect(response).toContain('No new note is added.');
    expect(response).not.toContain('Confirm type change');
  });

  it('HS2-6T8C1D renders the unsaved type choice instead of the stored kind', () => {
    const stored = String(NoteMetadataDialog({ state: { ...state, kind: 'regular' } }));
    expect(stored).toMatch(
      /name="note-metadata-kind"[^>]*value="regular"|value="regular"[^>]*name="note-metadata-kind"/u,
    );
    const chosen = String(NoteMetadataDialog({ state: { ...state, kind: 'regular', kindChoice: 'status' } }));
    expect(chosen).toMatch(
      /name="note-metadata-kind"[^>]*value="status"|value="status"[^>]*name="note-metadata-kind"/u,
    );
  });
});
