import { describe, expect, it } from 'vitest';

import { MarkdownEditor } from './markdown-editor';
import { MarkdownPreview } from './markdown-preview';
import { NoteComposer } from './note-composer';
import { TicketReader } from './ticket-reader';

describe('content components', () => {
  it('renders a controlled note composer with explicit submit and cancel states', () => {
    const empty = String(NoteComposer({ value: '' }));
    expect(empty).toContain('data-action="create-note-form"');
    expect(empty).toContain('data-action="cancel-new-note"');
    expect(empty).toContain('disabled');
    const ready = String(NoteComposer({ value: 'Ready' }));
    expect(ready).not.toContain('disabled');
  });
  it('renders Markdown source, preview, dirty state, and expansion as explicit states', () => {
    const source = String(MarkdownEditor({ value: '## Goal', mode: 'write', dirty: true }));
    expect(source).toContain('textarea');
    expect(source).toContain('Saving changes');
    expect(source).not.toContain('data-action="save-markdown"');
    expect(source).not.toContain('data-action="cancel-markdown-edit"');
    expect(source).toContain('data-lucide="maximize-2"');
    const preview = String(MarkdownEditor({ value: '## Goal', mode: 'preview', expanded: true }));
    expect(preview).toContain('data-component="markdown-preview"');
    expect(preview).toContain('data-expanded="true"');
    expect(preview).toContain('data-lucide="minimize-2"');
    const empty = String(MarkdownEditor({ value: '', mode: 'preview' }));
    expect(empty).toContain('data-empty="true"');
    expect(empty).toContain('Click to add Markdown.');
  });

  it('projects GFM Markdown while escaping raw HTML and unsafe links', () => {
    const markup = String(MarkdownPreview({ source: '# Title\n\n**bold** and `code`\n\n| A | B |\n| - | - |\n| 1 | 2 |\n\n[unsafe](javascript:alert(1))\n\n<script>alert(1)</script>' }));
    expect(markup).toContain('<h1');
    expect(markup).toContain('<strong>bold</strong>');
    expect(markup).toContain('<code>code</code>');
    expect(markup).toContain('<table>');
    expect(markup).toContain('href="#"');
    expect(markup).toContain('&lt;script&gt;alert(1)&lt;/script&gt;');
  });

  it('derives the TicketReader note count and reuses NoteCard', () => {
    const markup = String(TicketReader({ slug: 'HS2-TEST', title: 'Reader', status: 'started', priority: 'high', category: 'feature', tags: ['client'], details: 'Details', notes: [{ id: 'one', kind: 'regular', author: 'Codex', time: 'Now', body: 'Done' }] }));
    expect(markup).toContain('HS2-TEST');
    expect(markup).toContain('data-component="note-card"');
    expect(markup).toContain('<span>1</span>');
    expect(markup).toContain('role="dialog"');
    expect(markup).toContain('data-presentation="reader"');
    expect(markup).toContain('data-inspector-tab="attachments"');
  });
});
