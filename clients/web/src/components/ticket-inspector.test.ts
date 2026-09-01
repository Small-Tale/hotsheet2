import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

import { TicketInspector } from './ticket-inspector';

const base = { slug: 'HS2-TEST', title: 'Inspect this ticket', status: 'started' as const, priority: 'high' as const, category: 'feature', tags: ['client'], details: 'Readable details.' };

describe('TicketInspector', () => {
  it('renders each public tab without changing ticket identity', () => {
    for (const tab of ['info', 'timeline', 'attachments'] as const) {
      const markup = String(TicketInspector({ ...base, activeTab: tab }));
      expect(markup).toContain('HS2-TEST');
      expect(markup).toContain(`data-inspector-tab="${tab}" aria-label="${tab === 'info' ? 'Info' : tab === 'timeline' ? 'Timeline' : 'Attachments'}" aria-current="page"`);
      expect(markup).toContain('aria-label="Hide inspector"');
      expect(markup).toContain('data-lucide="panel-right-close"');
      expect(markup).toContain('data-component="toolbar-text" data-size="small">HS2-TEST');
      expect(markup).toContain('data-appearance="borderless"');
      if (tab === 'info') {
        expect(markup.match(/<wa-option value="feature"/g)).toHaveLength(1);
        expect(markup).toContain('data-component="ticket-notes"');
        expect(markup).toContain('data-component="markdown-preview"');
      }
    }
  });

  it('uses the same capability surface at reader scale with dialog close semantics', () => {
    const markup = String(TicketInspector({ ...base, presentation: 'reader', notes: [{ id: 'one', kind: 'regular', author: 'Codex', time: 'Now', body: 'Done' }] }));
    expect(markup).toContain('data-presentation="reader"');
    expect(markup).toContain('data-action="close-ticket-reader"');
    expect(markup).toContain('data-lucide="x"');
    expect(markup).toContain('data-component="note-card"');
    expect(markup).toContain('data-action="edit-ticket-reader"');
    expect(markup).not.toContain('data-action="edit-markdown"');
    const editing = String(TicketInspector({ ...base, presentation: 'reader', readerEditing: true, detailsMode: 'write', notes: [{ id: 'one', kind: 'regular', author: 'Codex', time: 'Now', body: 'Done' }] }));
    expect(editing).toContain('name="markdown-source"');
    expect(editing).toContain('data-edit-on-double-click="true"');
    expect(editing).toContain('aria-label="Editing ticket"');
    expect(editing).toContain('aria-pressed disabled');
  });

  it('shows a feedback-needed banner only when the ticket is waiting on the user', () => {
    expect(String(TicketInspector({ ...base }))).not.toContain('ticket-inspector__feedback');
    const waiting = String(TicketInspector({ ...base, feedbackNeeded: true }));
    expect(waiting).toContain('ticket-inspector__feedback');
    expect(waiting).toContain('Waiting on your feedback');
    expect(waiting).toContain('circle-alert');
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

  it('hides the Up Next action for ineligible lifecycle states', () => {
    expect(String(TicketInspector({ ...base }))).toContain('data-action="toggle-inspector-up-next"');
    expect(String(TicketInspector({ ...base, status: 'completed' }))).not.toContain('data-action="toggle-inspector-up-next"');
    expect(String(TicketInspector({ ...base, upNextEligible: false }))).not.toContain('data-action="toggle-inspector-up-next"');
  });
});
