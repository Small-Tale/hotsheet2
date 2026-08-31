import { describe, expect, it } from 'vitest';

import { NoteCard } from './note-card';

describe('NoteCard', () => {
  it.each([
    ['regular', 'message-square-text', 'Note'],
    ['status', 'refresh-cw', 'Status update'],
    ['feedback_needed', 'circle-alert', 'Feedback needed'],
    ['feedback_draft', 'file-pen-line', 'Feedback draft'],
    ['activity', 'activity', 'Activity'],
  ] as const)('renders the complete %s presentation', (kind, icon, label) => {
    const markup = String(NoteCard({ id: kind, kind, author: 'Codex', time: 'Now', body: 'Body' }));
    expect(markup).toContain(`data-kind="${kind}"`);
    expect(markup).toContain(`data-lucide="${icon}"`);
    expect(markup).toContain(label);
    expect(markup).toContain('Codex');
    expect(markup).toContain('Body');
  });

  it('renders a controlled editor only when editing is requested', () => {
    const markup = String(NoteCard({ id: 'editable', kind: 'regular', author: 'Codex', time: 'Now', body: 'Saved body', editing: true, draft: 'Draft body' }));
    expect(markup).toContain('note-card--editing');
    expect(markup).toContain('aria-label="Note body"');
    expect(markup).toContain('Draft body');
    expect(markup).toContain('Changes save automatically');
    expect(markup).not.toContain('data-action="save-note-edit"');
    expect(markup).not.toContain('data-action="cancel-note-edit"');
  });

  it('exposes a discoverable edit action', () => {
    const markup = String(NoteCard({ id: 'actions', kind: 'regular', author: 'Codex', time: 'Now', body: 'Body' }));
    expect(markup).toContain('data-edit-on-double-click="true"');
    expect(markup).toContain('aria-label="Edit note"');
    expect(markup).not.toContain('open-ticket-reader');
  });

  it('uses kind-driven reader behavior instead of launch-point editing', () => {
    const regular = String(NoteCard({ id: 'regular', kind: 'regular', author: 'Codex', time: 'Now', body: 'Read only', readerMode: true }));
    expect(regular).not.toContain('data-edit-on-double-click');
    expect(regular).not.toContain('aria-label="Edit note"');
    const needed = String(NoteCard({ id: 'needed', kind: 'feedback_needed', author: 'Codex', time: 'Now', body: 'Please answer', readerMode: true }));
    expect(needed).toContain('Please answer');
    expect(needed).toContain('aria-label="Feedback response"');
    expect(needed).toContain('data-note-response="true"');
    expect(needed).toContain('Respond');
    const draft = String(NoteCard({ id: 'draft', kind: 'feedback_draft', author: 'You', time: 'Now', body: 'Continue me', readerMode: true }));
    expect(draft).toContain('aria-label="Note body"');
    expect(draft).toContain('Continue me');
    expect(draft).toContain('Submit');
  });
});
