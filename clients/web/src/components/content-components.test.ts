import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

import { MarkdownEditor } from './markdown-editor';
import { MarkdownPreview } from './markdown-preview';
import { NoteComposer } from './note-composer';
import { TicketNotes } from './ticket-notes';
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
  it('offers the visible Add note action when the note list is empty', () => {
    const markup = String(TicketNotes({ notes: [] }));
    expect(markup).toContain('No notes added.');
    expect(markup).toContain('class="ticket-notes__add"');
    expect(markup).toContain('>Add note</button>');
  });
  it('places a focused new-note composer after existing notes', () => {
    const markup=String(TicketNotes({ notes: [{ id: 'one', kind: 'regular', author: 'Codex', time: 'Now', body: 'Existing' }], composing: true }));
    expect(markup.indexOf('data-component="note-card"')).toBeLessThan(markup.indexOf('data-component="note-composer"'));
  });
  it('offers Respond to Feedback only below the active inspector feedback note', () => {
    const notes = [
      { id: 'old', kind: 'feedback_needed' as const, author: 'Codex', time: 'Earlier', body: 'Old ask' },
      { id: 'active', kind: 'feedback_needed' as const, author: 'Codex', time: 'Now', body: 'Current ask' },
    ];
    const inspector = String(TicketNotes({ notes }));
    expect(inspector.match(/data-action="respond-to-feedback"/g)).toHaveLength(1);
    expect(inspector).toMatch(/data-note-id="active"[^]*data-action="respond-to-feedback" data-note-id="active"/);
    expect(String(TicketNotes({ notes, readerMode: true }))).not.toContain('respond-to-feedback');
    expect(String(TicketNotes({ notes: [...notes, { id: 'answer', kind: 'regular', author: 'You', time: 'Later', body: 'Answered' }] }))).not.toContain('respond-to-feedback');
  });
  it('renders Markdown source, preview, and expansion as explicit states without a save footer', () => {
    const source = String(MarkdownEditor({ value: '## Goal', mode: 'write', dirty: true }));
    expect(source).toContain('textarea');
    // No standalone save footer / autosave hint — editing autosaves silently (HS2-N7NBFG).
    expect(source).not.toContain('<footer>');
    expect(source).not.toContain('Saving changes');
    expect(source).not.toContain('Changes saved');
    expect(source).not.toContain('data-action="save-markdown"');
    expect(source).not.toContain('data-action="cancel-markdown-edit"');
    expect(source).toContain('data-lucide="maximize-2"');
    const embedded = String(MarkdownEditor({ value: '## Goal', mode: 'write', dirty: true, appearance: 'embedded' }));
    expect(embedded).not.toContain('<footer>');
    const css = readFileSync(resolve(import.meta.dirname, 'markdown-editor.css'), 'utf8');
    expect(css).toMatch(/markdown-editor--embedded \{[^}]*grid-template-rows: minmax\(0, 1fr\);[^}]*gap: 0;/);
    expect(css).toMatch(/markdown-editor--embedded \.markdown-editor__surface \{[^}]*display: grid;[^}]*padding: 0;[^}]*overflow: visible;/);
    expect(css).toMatch(/markdown-editor--embedded \.markdown-editor__preview \{[^}]*padding: \.75rem;/);
    expect(css).toMatch(/markdown-editor--embedded \.markdown-editor__surface textarea \{[^}]*display: block;[^}]*box-sizing: border-box;[^}]*height: auto;[^}]*padding: \.75rem;[^}]*resize: vertical/);
    const panelCss = readFileSync(resolve(import.meta.dirname, 'ticket-inspector-panel.css'), 'utf8');
    expect(panelCss).toMatch(/ticket-inspector__details-surface \{[^}]*padding: 0;/);
    expect(embedded).not.toContain('Saving changes');
    const preview = String(MarkdownEditor({ value: '## Goal', mode: 'preview', expanded: true }));
    expect(preview).toContain('data-component="markdown-preview"');
    expect(preview).toContain('data-expanded="true"');
    expect(preview).toContain('data-lucide="minimize-2"');
    const empty = String(MarkdownEditor({ value: '', mode: 'preview' }));
    expect(empty).toContain('data-empty="true"');
    expect(empty).toContain('Click to add Markdown.');
  });

  it('projects GFM Markdown with safe new-tab links while escaping raw HTML and unsafe protocols', () => {
    const markup = String(MarkdownPreview({ source: '# Title\n\n**bold** and `code`\n\n| A | B |\n| - | - |\n| 1 | 2 |\n\n[safe](/guide "Guide") and <https://example.com/docs>\n\n[unsafe](javascript:alert(1))\n\n<script>alert(1)</script>' }));
    expect(markup).toContain('<h1');
    expect(markup).toContain('<strong>bold</strong>');
    expect(markup).toContain('<code>code</code>');
    expect(markup).toContain('<table>');
    expect(markup).toContain('href="/guide" target="_blank" rel="noopener noreferrer" title="Guide"');
    expect(markup).toContain('href="https://example.com/docs" target="_blank" rel="noopener noreferrer"');
    expect(markup).toContain('href="#" target="_blank" rel="noopener noreferrer"');
    expect(markup).toContain('&lt;script&gt;alert(1)&lt;/script&gt;');
    const css = readFileSync(resolve(import.meta.dirname, 'markdown-preview.css'), 'utf8');
    expect(css).toContain('.markdown-preview p { margin: var(--wa-space-m) 0; }');
    expect(css).toMatch(/blockquote \{[^}]*margin-inline: 0;[^}]*border-left: 2px[^}]*font-size: var\(--wa-font-size-xs\);[^}]*line-height: 1\.5;/);
    expect(css).toMatch(/blockquote :is\(h1, h2, h3, h4, h5, h6\) \{ font-size: var\(--wa-font-size-xs\); \}/);
    expect(css).toMatch(/\.markdown-preview img \{[^}]*display: block;[^}]*height: auto;/);
    expect(css).toMatch(/\.markdown-preview__attachment-image \{[^}]*width: fit-content;[^}]*height: auto;[^}]*overflow: hidden;/);
  });

  it('renders filename attachment references as host actions and inline gallery images',()=>{
    const context={baseUrl:'/project-api/demo',checkout:'checkout',ticket:'HS2-LOCAL'};
    const markup=String(MarkdownPreview({source:'`attachment:report.pdf`\n\n`attachment:[HS2-OTHER]screen shot.svg`',attachmentContext:context}));
    expect(markup).toContain('data-action="open-referenced-attachment"');
    expect(markup).toContain('data-attachment-name="report.pdf"');
    expect(markup).toContain('data-action="open-attachment-gallery"');
    expect(markup).toContain('data-attachment-ticket="HS2-OTHER"');
    expect(markup).toContain('/tickets/HS2-OTHER/attachments/by-name/screen%20shot.svg');
  });

  it('derives the TicketReader note count and reuses NoteCard', () => {
    const markup = String(TicketReader({ slug: 'HS2-TEST', title: 'Reader', status: 'started', priority: 'high', category: 'feature', tags: ['client'], details: 'Details', notes: [{ id: 'one', kind: 'regular', author: 'Codex', time: 'Now', body: 'Done' }] }));
    expect(markup).toContain('HS2-TEST');
    expect(markup).toContain('data-component="note-card"');
    expect(markup).toContain('<span>1</span>');
    expect(markup).toContain('role="dialog"');
    expect(markup).toContain('data-presentation="reader"');
    expect(markup).toContain('data-inspector-tab="attachments"');
    const readerCss = readFileSync(resolve(import.meta.dirname, 'ticket-reader.css'), 'utf8');
    expect(readerCss).toMatch(/\.ticket-reader \{[^}]*height: calc\(100vh - 3rem\);/);
    const shellCss = readFileSync(resolve(import.meta.dirname, '..', 'style.css'), 'utf8');
    expect(shellCss).toMatch(/\.ticket-reader-backdrop \{[^}]*padding: 1\.5rem;/);
  });

  it('keeps the feedback catchall at half the ordinary note-editor minimum height',()=>{const css=readFileSync(resolve(import.meta.dirname,'note-card.css'),'utf8');expect(css).toMatch(/textarea\[data-note-response="true"\] \{ min-height: 2\.5rem; \}/)});
});
