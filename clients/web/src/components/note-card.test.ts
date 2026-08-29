import { describe, expect, it } from 'vitest';
import { NoteCard } from './note-card';

describe('NoteCard', () => {
  it.each([
    ['regular', 'message-square-text', 'Note'],
    ['status', 'refresh-cw', 'Status update'],
    ['feedback_needed', 'circle-alert', 'Feedback needed'],
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
    expect(markup).toContain('data-action="save-note-edit"');
    expect(markup).toContain('data-action="cancel-note-edit"');
  });
});
