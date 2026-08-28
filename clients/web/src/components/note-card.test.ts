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
});
