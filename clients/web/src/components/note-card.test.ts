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
    // A regular note editor autosaves silently — no redundant "Changes save automatically"
    // hint and no Save/Cancel buttons (HS2-5NJRV9).
    expect(markup).not.toContain('Changes save automatically');
    expect(markup).not.toContain('data-action="save-note-edit"');
    expect(markup).not.toContain('data-action="cancel-note-edit"');
  });

  it('prefills direct feedback-note edits while keeping reader responses empty', () => {
    const editing = String(NoteCard({ id: 'feedback-edit', kind: 'feedback_needed', author: 'Codex', time: 'Now', body: 'Existing feedback question', editing: true }));
    expect(editing).toContain('aria-label="Note body"');
    expect(editing).toContain('Existing feedback question');
    const responding = String(NoteCard({ id: 'feedback-response', kind: 'feedback_needed', author: 'Codex', time: 'Now', body: 'Existing feedback question', readerMode: true }));
    expect(responding).toContain('aria-label="Feedback response"');
    expect(responding).toContain('<textarea name="note-body" data-note-id="feedback-response" data-note-response="true" aria-label="Feedback response"></textarea>');
  });

  it('exposes content editing without a redundant edit button', () => {
    const markup = String(NoteCard({ id: 'actions', kind: 'regular', author: 'Codex', time: 'Now', body: 'Body' }));
    expect(markup).toContain('data-edit-on-double-click="true"');
    expect(markup).toContain('aria-label="Edit note"');
    expect(markup).not.toContain('data-action="edit-note"');
    expect(markup).not.toContain('open-ticket-reader');
  });

  it('renders note Markdown through the shared safe new-tab boundary', () => {
    const markup = String(NoteCard({ id: 'link', kind: 'regular', author: 'Codex', time: 'Now', body: 'Read the [runbook](/docs/runbook).' }));
    expect(markup).toContain('href="/docs/runbook" target="_blank" rel="noopener noreferrer"');
  });

  it('renders feedback-needed notes with the same Markdown support as regular notes', () => {
    const markup = String(NoteCard({ id: 'feedback-markdown', kind: 'feedback_needed', author: 'Codex', time: 'Now', body: 'Choose one:\n\n1. **Keep** this\n2. Use `that`' }));
    expect(markup).toContain('<ol>');
    expect(markup).toContain('<strong>Keep</strong>');
    expect(markup).toContain('<code>that</code>');
  });

  it('keeps regular reader notes directly editable while preserving feedback response behavior', () => {
    const regular = String(NoteCard({ id: 'regular', kind: 'regular', author: 'Codex', time: 'Now', body: 'Read only', readerMode: true }));
    expect(regular).toContain('data-edit-on-double-click="true"');
    expect(regular).toContain('aria-label="Edit note"');
    expect(regular).not.toContain('data-action="edit-note"');
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
