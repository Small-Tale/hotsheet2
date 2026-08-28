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
      if (tab === 'info') expect(markup.match(/<wa-option value="feature"/g)).toHaveLength(1);
    }
  });
});
