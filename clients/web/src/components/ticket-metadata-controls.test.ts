import { describe, expect, it } from 'vitest';
import { TicketCategorySelect } from './ticket-category-select';
import { TicketPrioritySelect } from './ticket-priority-select';
import { TicketInfoPanel } from './ticket-info-panel';
import { TicketTimeline } from './ticket-timeline';
import { TicketAttachments } from './ticket-attachments';

describe('ticket metadata controls and inspector panels', () => {
  it('renders colored category icons and semantic priority icons', () => {
    const category = String(TicketCategorySelect({ name: 'category', value: 'bug' }));
    expect(category).toContain('data-lucide="bug"');
    expect(category).toContain('color:#ef4444');
    const priority = String(TicketPrioritySelect({ name: 'priority', value: 'urgent' }));
    expect(priority).toContain('data-lucide="chevrons-up"');
    expect(priority).toContain('data-lucide="minus"');
  });

  it('renders inspector sections independently of the inspector shell', () => {
    expect(String(TicketInfoPanel({ status: 'started', priority: 'high', category: 'feature', tags: ['ux'], details: 'Details' }))).toContain('data-component="ticket-info-panel"');
    const timeline = String(TicketTimeline({ entries: [{ id: 'one', time: 'Now', text: 'One note' }] }));
    expect(timeline.match(/<li/g)).toHaveLength(1);
    expect(timeline).toContain('1 note total');
    const attachments = String(TicketAttachments({ attachments: [{ id: 'one', name: 'one.png' }] }));
    expect(attachments.match(/data-attachment-id=/g)).toHaveLength(1);
    expect(attachments).toContain('1 attachment total');
  });
});
