import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

import { TicketInspector } from './ticket-inspector';

const base = { slug: 'HS2-TEST', title: 'Inspect this ticket', status: 'started' as const, priority: 'high' as const, category: 'feature', tags: ['client'], details: 'Readable details.' };

describe('TicketInspector', () => {
  it('allows the sidebar title to wrap without a line cap', () => {
    const css = readFileSync(resolve(import.meta.dirname, 'ticket-inspector.css'), 'utf8');
    const titleRule = css.match(/\.ticket-inspector__header h1 \{([^}]*)\}/)?.[1] ?? '';
    expect(titleRule).toContain('overflow-wrap: anywhere');
    expect(titleRule).not.toContain('line-clamp');
  });

  it('renders each public tab without changing ticket identity', () => {
    for (const tab of ['info', 'timeline', 'code-review', 'attachments'] as const) {
      const markup = String(TicketInspector({ ...base, activeTab: tab }));
      expect(markup).toContain('HS2-TEST');
      expect(markup).toContain(`data-inspector-tab="${tab}" aria-label="${tab === 'info' ? 'Info' : tab === 'timeline' ? 'Timeline' : tab === 'code-review' ? 'Code Review' : 'Attachments'}" aria-current="page"`);
      expect(markup).toContain('aria-label="Hide inspector"');
      expect(markup).toContain('data-lucide="panel-right-close"');
      expect(markup).toContain('data-component="toolbar-text" data-size="small">HS2-TEST');
      expect(markup).toContain('data-action="copy-ticket-slug" aria-label="Copy ticket number HS2-TEST"');
      expect(markup).toContain('data-appearance="borderless"');
      if (tab === 'info') {
        expect(markup.match(/<wa-option value="feature"/g)).toHaveLength(1);
        expect(markup).toContain('data-component="ticket-notes"');
        expect(markup).toContain('data-component="markdown-preview"');
      }
      if (tab === 'code-review') expect(markup).toContain('data-lucide="message-square-code"');
    }
  });

  it('changes only the Code Review segment icon', () => {
    const markup = String(TicketInspector({ ...base, activeTab: 'code-review', codeReview: { difftool: 'Glassbox', truncated: false, ranges: [], commits: [{ sha: 'abcdef', short_sha: 'abcdef', subject: 'Review action', committed_at: '2026-09-02T08:00:00Z' }] } }));
    expect(markup).toContain('data-inspector-tab="code-review"');
    expect(markup).toContain('data-lucide="message-square-code"');
    expect(markup).toContain('ticket-code-review__graph');
    expect(markup).toContain('data-lucide="git-commit-horizontal"');
    expect(markup).toContain('data-lucide="external-link"');
  });

  it('uses the same capability surface at reader scale with dialog close semantics', () => {
    const markup = String(TicketInspector({ ...base, presentation: 'reader', notes: [{ id: 'one', kind: 'regular', author: 'Codex', time: 'Now', body: 'Done' }] }));
    expect(markup).toContain('data-presentation="reader"');
    expect(markup).toContain('data-action="close-ticket-reader"');
    expect(markup).toContain('data-lucide="x"');
    expect(markup).toContain('data-lucide="a-large-small"');
    expect(markup).toContain('aria-label="Use large reader text size"');
    expect(markup).toMatch(/data-button-appearance="push"[^>]*data-single="true"[^>]*><button[^>]*data-action="toggle-reader-text-size"/);
    const largeMarkup = String(TicketInspector({ ...base, presentation: 'reader', largeText: true }));
    expect(largeMarkup).toContain('aria-label="Use standard reader text size" aria-pressed="true"');
    expect(markup).toContain('data-component="note-card"');
    expect(markup).not.toContain('data-action="edit-ticket-reader"');
    expect(markup).toContain('data-action="edit-markdown"');
    expect(markup).toContain('data-edit-on-double-click="true"');
    expect(markup).not.toContain('data-action="edit-note"');
    expect(markup).toContain('popoverTarget="ticket-tag-reader-hs2-test"');
    expect(String(TicketInspector({ ...base }))).toContain('popoverTarget="ticket-tag-sidebar-hs2-test"');
    const editing = String(TicketInspector({ ...base, presentation: 'reader', detailsMode: 'write', notes: [{ id: 'one', kind: 'regular', author: 'Codex', time: 'Now', body: 'Done' }] }));
    expect(editing).toContain('name="markdown-source"');
    expect(editing).toContain('data-edit-on-double-click="true"');
  });

  it('shows a feedback-needed banner only when the ticket is waiting on the user', () => {
    expect(String(TicketInspector({ ...base }))).not.toContain('ticket-inspector__feedback');
    const waiting = String(TicketInspector({ ...base, feedbackNeeded: true }));
    expect(waiting).toContain('ticket-inspector__feedback');
    expect(waiting).toContain('data-needs-review="true"');
    expect(waiting).toContain('Needs review');
    expect(waiting).toContain('circle-alert');
    const css = readFileSync(resolve(import.meta.dirname, 'ticket-inspector.css'), 'utf8');
    expect(css).toMatch(/data-presentation="reader"\]\[data-needs-review="true"[^}]*var\(--hs-ticket-state-needs-review\)/);
  });

  it('renders the structured duplicate outcome and canonical ticket action', () => {
    const markup = String(TicketInspector({ ...base, status: 'completed', closeReason: 'duplicate', duplicateTarget: { id: 'target-id', label: 'HS2-TARGET' } }));
    expect(markup).toContain('data-close-reason="duplicate"');
    expect(markup).toContain('Duplicate of');
    expect(markup).toContain('data-action="open-duplicate-target" data-target-id="target-id"');
    expect(markup).toContain('data-lucide="copy-x"');
  });

  it('renders marked description choices as the reader feedback surface',()=>{
    const details='FEEDBACK NEEDED: Which direction?\n\nCHOICE:\n- Keep **A**\n- Use `B`';
    const sidebar=String(TicketInspector({...base,details,feedbackNeeded:true}));
    expect(sidebar).toContain('data-note-id="ticket-details"');
    expect(sidebar).toContain('data-feedback-needed="true"');
    expect(sidebar).toContain('Respond to Feedback');
    const reader=String(TicketInspector({...base,details,feedbackNeeded:true,presentation:'reader'}));
    expect(reader).toContain('data-details-feedback="true"');
    expect(reader).toContain('ticket-inspector__details-feedback-header');
    expect(reader).toContain('Feedback needed');
    expect(reader).toContain('data-lucide="circle-alert"');
    expect(reader.match(/data-action="toggle-feedback-choice"/g)).toHaveLength(2);
    expect(reader).toContain('aria-label="Feedback response"');
    expect(reader).not.toContain('CHOICE:');
    const css=readFileSync(resolve(import.meta.dirname,'ticket-inspector-panel.css'),'utf8');
    expect(css).toMatch(/details-surface\[data-feedback-needed="true"\] \{[^}]*padding: \.85rem 1rem;[^}]*warning-border-normal[^}]*warning-fill-quiet/);
  });

  it('shows a derived attachment count on the attachments segment', () => {
    const markup = String(TicketInspector({ ...base, attachments: [{ id: 'one', name: 'one.png' }, { id: 'two', name: 'two.md' }] }));
    expect(markup).toContain('aria-label="Attachments, 2"');
    expect(markup).toContain('ticket-inspector__tab-count');
    expect(markup).toContain('>2</span>');
    expect(String(TicketInspector({ ...base, attachments: [] }))).not.toContain('ticket-inspector__tab-count');
  });

  it('keeps attachment names shrinkable while preserving the compact menu trigger', () => {
    const css = readFileSync(resolve(import.meta.dirname, 'ticket-inspector-panel.css'), 'utf8');
    expect(css).toContain('.ticket-inspector__attachment { display: flex; width: 100%; min-width: 0;');
    expect(css).toContain('.ticket-inspector__attachment > span { min-width: 0; overflow: hidden; flex: 1;');
    expect(css).toContain('.ticket-inspector__attachment-menu { display: inline-grid; width: 1.75rem; height: 1.75rem; margin-left: auto;');
  });

  it('contains metadata and ticket content within narrow inspector bounds', () => {
    const inspectorCss = readFileSync(resolve(import.meta.dirname, 'ticket-inspector.css'), 'utf8');
    const panelCss = readFileSync(resolve(import.meta.dirname, 'ticket-inspector-panel.css'), 'utf8');
    const noteCss = readFileSync(resolve(import.meta.dirname, 'note-card.css'), 'utf8');
    expect(inspectorCss).toMatch(/\.ticket-inspector \{[^}]*min-width: 0;[^}]*max-width: 100%/);
    expect(inspectorCss).toContain('grid-template-columns: repeat(4, minmax(0, 1fr))');
    expect(panelCss).toMatch(/\.ticket-inspector__content \{[^}]*min-width: 0;[^}]*overflow-x: hidden/);
    expect(panelCss).toContain('grid-template-columns: repeat(2, minmax(0, 1fr))');
    expect(panelCss).toContain('.ticket-inspector__metadata > .select { width: 100%; min-width: 0; }');
    expect(noteCss).toMatch(/\.note-card__body \{[^}]*overflow-wrap: anywhere/);
    expect(noteCss).toMatch(/\.note-card\[data-kind="activity"\] \{[^}]*background: transparent/);
    expect(noteCss).toMatch(/\.note-card\[data-kind="activity"\] \.note-card__body \{[^}]*font-size: var\(--wa-font-size-xs\)/);
    expect(inspectorCss).toContain('@container (max-width: 52rem) { .ticket-inspector__tab-label { display: none; } }');
  });

  it('uses the compact eight pixel inspector gutter without duplicating its tab gap', () => {
    const inspectorCss = readFileSync(resolve(import.meta.dirname, 'ticket-inspector.css'), 'utf8');
    const panelCss = readFileSync(resolve(import.meta.dirname, 'ticket-inspector-panel.css'), 'utf8');
    expect(inspectorCss).toMatch(/\.ticket-inspector__tabs \{[^}]*margin: 0 \.5rem 1rem;/);
    expect(panelCss).toMatch(/\.ticket-inspector__content \{[^}]*padding: 0 \.5rem \.5rem;/);
  });

  it('hides the Up Next action for ineligible lifecycle states', () => {
    expect(String(TicketInspector({ ...base }))).toContain('data-action="toggle-inspector-up-next"');
    expect(String(TicketInspector({ ...base, status: 'completed' }))).not.toContain('data-action="toggle-inspector-up-next"');
    expect(String(TicketInspector({ ...base, upNextEligible: false }))).not.toContain('data-action="toggle-inspector-up-next"');
  });
});
