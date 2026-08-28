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
  });
});
