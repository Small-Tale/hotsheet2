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
    expect(markup).toContain('data-component="note-card"');
    expect(markup).not.toContain('data-action="edit-ticket-reader"');
    expect(markup).toContain('data-action="edit-markdown"');
    expect(markup).toContain('data-edit-on-double-click="true"');
    expect(markup).not.toContain('data-action="edit-note"');
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
    expect(css).toMatch(/data-needs-review="true"[^}]*var\(--hs-ticket-state-needs-review\)/);
  });

  it('shows a derived attachment count on the attachments segment', () => {
    const markup = String(TicketInspector({ ...base, attachments: [{ id: 'one', name: 'one.png' }, { id: 'two', name: 'two.md' }] }));
    expect(markup).toContain('aria-label="Attachments, 2"');
    expect(markup).toContain('ticket-inspector__tab-count');
    expect(markup).toContain('>2</span>');
    expect(String(TicketInspector({ ...base, attachments: [] }))).not.toContain('ticket-inspector__tab-count');
  });

  it('keeps attachment names shrinkable while preserving the action group', () => {
    const css = readFileSync(resolve(import.meta.dirname, 'ticket-inspector-panel.css'), 'utf8');
    expect(css).toContain('.ticket-inspector__attachment { display: flex; width: 100%; min-width: 0;');
    expect(css).toContain('span:nth-child(2) { min-width: 0; overflow: hidden; flex: 1;');
    expect(css).toContain('.ticket-inspector__attachment-actions { display: inline-flex; margin-left: auto; flex: none;');
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

  it('puts the tab-to-content gap on the segmented control without duplicate content padding', () => {
    const inspectorCss = readFileSync(resolve(import.meta.dirname, 'ticket-inspector.css'), 'utf8');
    const panelCss = readFileSync(resolve(import.meta.dirname, 'ticket-inspector-panel.css'), 'utf8');
    expect(inspectorCss).toMatch(/\.ticket-inspector__tabs \{[^}]*margin: 0 1rem 1rem;/);
    expect(panelCss).toMatch(/\.ticket-inspector__content \{[^}]*padding: 0 1rem 1rem;/);
  });

  it('hides the Up Next action for ineligible lifecycle states', () => {
    expect(String(TicketInspector({ ...base }))).toContain('data-action="toggle-inspector-up-next"');
    expect(String(TicketInspector({ ...base, status: 'completed' }))).not.toContain('data-action="toggle-inspector-up-next"');
    expect(String(TicketInspector({ ...base, upNextEligible: false }))).not.toContain('data-action="toggle-inspector-up-next"');
  });
});
