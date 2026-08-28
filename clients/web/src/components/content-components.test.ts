import { describe, expect, it } from 'vitest';
import { MarkdownEditor } from './markdown-editor';
import { MarkdownPreview } from './markdown-preview';
import { TicketReader } from './ticket-reader';

describe('content components', () => {
  it('renders Markdown source, preview, dirty state, and expansion as explicit states', () => {
    const source = String(MarkdownEditor({ value: '## Goal', mode: 'write', dirty: true }));
    expect(source).toContain('textarea');
    expect(source).toContain('Unsaved changes');
    expect(source).toContain('data-lucide="maximize-2"');
    const preview = String(MarkdownEditor({ value: '## Goal', mode: 'preview', expanded: true }));
    expect(preview).toContain('data-component="markdown-preview"');
    expect(preview).toContain('data-expanded="true"');
    expect(preview).toContain('data-lucide="minimize-2"');
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
