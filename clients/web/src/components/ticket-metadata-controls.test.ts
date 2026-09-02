import { describe, expect, it } from 'vitest';

import { TicketAttachments } from './ticket-attachments';
import { TicketCategorySelect } from './ticket-category-select';
import { TicketInfoPanel } from './ticket-info-panel';
import { TicketPrioritySelect } from './ticket-priority-select';
import { TicketStatusMenu } from './ticket-status-menu';
import { TicketTimeline } from './ticket-timeline';

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
    expect(status).toContain('select select--custom-selected ticket-status-menu');
    expect(status).toContain('name="inspector-status"');
    expect(status).toContain('<span slot="start" class="select__custom-selected"><span class="status-badge status-badge--completed');
    expect(status).toContain('<wa-option value="verified"><span slot="start" class="select__icon"');
    expect(status).toContain('<wa-divider></wa-divider><wa-option value="backlog"');
    expect(status).toContain('<wa-option value="archive"');
    expect(status).toContain('data-lucide="badge-check"');
    expect(status.match(/data-lucide=/g)).toHaveLength(7);
  });

  it('renders inspector sections independently of the inspector shell', () => {
    const info = String(TicketInfoPanel({ status: 'started', priority: 'high', category: 'feature', tags: ['ux'], details: 'Details' }));
    expect(info).toContain('data-component="ticket-info-panel"');
    expect(info.match(/ticket-inspector__section-header/g)).toHaveLength(4);
    expect(info).toContain('data-action="edit-blocked-reason"');
    expect(info).toContain('Block ticket');
    expect(info).toContain('data-component="ticket-notes"');
    const timeline = String(TicketTimeline({ entries: [{ id: 'one', time: 'Now', title: 'One event', subtitle: 'Optional detail' }] }));
    expect(timeline.match(/<li/g)).toHaveLength(1);
    expect(timeline).toContain('One event');
    expect(timeline).toContain('Optional detail');
    expect(timeline).toContain('1 event total');
    const attachments = String(TicketAttachments({ attachments: [{ id: 'one', name: 'one.png' }] }));
    expect(attachments.match(/class="ticket-inspector__attachment"/g)).toHaveLength(1);
    expect(attachments).toContain('1 attachment total');
    expect(attachments).toContain('data-attachment-drop-target="true"');
    expect(attachments).toContain('aria-label="Browse and add attachments"');
    expect(attachments).toContain('aria-label="Drop or browse attachments"');
    expect(attachments).toContain('data-action="open-attachment"');
    expect(attachments).toContain('data-action="download-attachment"');
    expect(attachments).toContain('data-action="copy-attachment-reference"');
    expect(attachments).toContain('data-action="remove-attachment"');
    expect(attachments).toContain('data-lucide="external-link"');
    expect(attachments).toContain('data-lucide="download"');
    expect(attachments).toContain('data-lucide="clipboard"');
    expect(attachments).toContain('data-lucide="trash-2"');
    const unsupported = String(TicketAttachments({ attachments: [{ id: 'one', name: 'one.png' }], enabled: false }));
    expect(unsupported).toContain('does not support attachment actions');
    expect(unsupported).not.toContain('name="ticket-attachments"');
    expect(unsupported).not.toContain('data-action="remove-attachment"');
  });
});
