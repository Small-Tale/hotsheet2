import { describe, expect, it } from 'vitest';
import { TicketCategorySelect } from './ticket-category-select';
import { TicketPrioritySelect } from './ticket-priority-select';
import { TicketInfoPanel } from './ticket-info-panel';
import { TicketTimeline } from './ticket-timeline';
import { TicketAttachments } from './ticket-attachments';
import { TicketStatusMenu } from './ticket-status-menu';

describe('ticket metadata controls and inspector panels', () => {
  it('renders colored category icons and semantic priority icons', () => {
    const category = String(TicketCategorySelect({ name: 'category', value: 'bug' }));
    expect(category).toContain('data-component="select"');
    expect(category).toContain('data-lucide="bug"');
    expect(category).toContain('color:#ef4444');
    const priority = String(TicketPrioritySelect({ name: 'priority', value: 'urgent' }));
    expect(priority).toContain('data-lucide="chevrons-up"');
    expect(priority).toContain('data-lucide="minus"');
    const status = String(TicketStatusMenu({ value: 'completed' }));
    expect(status).toContain('aria-label="Change status, Completed"');
    expect(status).toContain('<button type="button" slot="trigger" class="status-badge');
    expect(status).not.toContain('<wa-button');
    expect(status).toContain('data-inspector-status="verified"');
    expect(status).not.toContain('with-caret');
  });

  it('renders inspector sections independently of the inspector shell', () => {
    const info = String(TicketInfoPanel({ status: 'started', priority: 'high', category: 'feature', tags: ['ux'], details: 'Details' }));
    expect(info).toContain('data-component="ticket-info-panel"');
    expect(info.match(/ticket-inspector__section-header/g)).toHaveLength(3);
    expect(info).toContain('data-component="ticket-notes"');
    const timeline = String(TicketTimeline({ entries: [{ id: 'one', time: 'Now', text: 'One note' }] }));
    expect(timeline.match(/<li/g)).toHaveLength(1);
    expect(timeline).toContain('1 note total');
    const attachments = String(TicketAttachments({ attachments: [{ id: 'one', name: 'one.png' }] }));
    expect(attachments.match(/data-attachment-id=/g)).toHaveLength(1);
    expect(attachments).toContain('1 attachment total');
    expect(attachments).toContain('data-attachment-drop-target="true"');
    expect(attachments).toContain('aria-label="Browse and add attachments"');
    expect(attachments).toContain('aria-label="Drop or browse attachments"');
  });
});
