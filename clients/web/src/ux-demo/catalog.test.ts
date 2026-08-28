import { describe, expect, it } from 'vitest';
import { createDevApp } from '../dev-server';
import { demosUsing, demoCatalog, findDemo, flattenCatalog } from './catalog';
import { resetStatusBadgeDemo, statusBadgeSettings } from './status-badge-demo';
import { resetTagChipDemo, tagChipSettings } from './tag-chip-demo';
import { resetTicketRowDemo, ticketRowSettings } from './ticket-row-demo';

describe('UX demo catalog', () => {
  it('has unique routes and the implemented component set', () => {
    const entries = flattenCatalog(demoCatalog);
    expect(new Set(entries.map(entry => entry.id)).size).toBe(entries.length);
    expect(entries.filter(entry => entry.implemented).map(entry => entry.id)).toEqual(['app-shell', 'project-sidebar', 'project-summary', 'repository-summary', 'view-navigation', 'command-navigation', 'drive-control', 'workspace-header', 'project-tab', 'project-tabs', 'resizable-region', 'connection-state-banner', 'quick-ticket-composer', 'ticket-list', 'ticket-row', 'ticket-board', 'ticket-board-column', 'ticket-inspector', 'ticket-info-panel', 'ticket-timeline', 'ticket-attachments', 'ticket-category-select', 'ticket-priority-select', 'ticket-status-menu', 'status-badge', 'tag-chip', 'toolbar-control-group']);
    expect(findDemo('tag-chip')?.name).toBe('TagChip');
    expect(findDemo('ticket-row')?.uses).toEqual(['status-badge', 'tag-chip']);
    expect(demosUsing('tag-chip').map(entry => entry.id)).toEqual(['ticket-row', 'ticket-info-panel']);
    expect(demosUsing('ticket-row').map(entry => entry.id)).toEqual(['ticket-list', 'ticket-board-column']);
    expect(findDemo('ticket-board')?.uses).toEqual(['ticket-board-column']);
    expect(findDemo('workspace-header')?.uses).toEqual(['toolbar-control-group', 'ticket-list', 'ticket-board']);
    expect(demosUsing('toolbar-control-group').map(entry => entry.id)).toEqual(['workspace-header']);
    expect(findDemo('project-tabs')?.uses).toEqual(['project-tab']);
    expect(demosUsing('project-tab').map(entry => entry.id)).toEqual(['project-tabs']);
    expect(findDemo('ticket-inspector')?.uses).toEqual(['ticket-info-panel', 'ticket-timeline', 'ticket-attachments']);
    expect(entries.flatMap(entry => entry.uses ?? []).every(id => findDemo(id))).toBe(true);
  });

  it('records the planned ProjectSidebar composition', () => {
    expect(findDemo('project-sidebar')?.uses).toEqual(['project-summary', 'repository-summary', 'view-navigation', 'command-navigation', 'drive-control']);
    expect(demosUsing('drive-control').map(entry => entry.id)).toEqual(['project-sidebar']);
  });

  it('serves UX markup only when development is explicitly enabled', async () => {
    const dev = await createDevApp(true).request('/ux-demo');
    expect(dev.status).toBe(200);
    expect(await dev.text()).toContain('/src/ux-demo/main.tsx');
    expect((await createDevApp(false).request('/ux-demo')).status).toBe(404);
  });

  it('resets every canonical TagChip demo setting', () => {
    tagChipSettings.label.value = 'changed';
    tagChipSettings.variant.value = 'danger';
    tagChipSettings.appearance.value = 'accent';
    tagChipSettings.size.value = 'large';
    tagChipSettings.removable.value = false;
    tagChipSettings.pill.value = false;
    tagChipSettings.disabled.value = true;
    tagChipSettings.event.value = 'Changed';
    resetTagChipDemo();
    expect({
      label: tagChipSettings.label.value, variant: tagChipSettings.variant.value,
      appearance: tagChipSettings.appearance.value, size: tagChipSettings.size.value,
      removable: tagChipSettings.removable.value, pill: tagChipSettings.pill.value,
      disabled: tagChipSettings.disabled.value, event: tagChipSettings.event.value,
    }).toEqual({
      label: 'needs-design', variant: 'neutral', appearance: 'filled', size: 'small',
      removable: true, pill: false, disabled: false, event: 'No actions yet',
    });
  });

  it('resets every canonical StatusBadge demo setting', () => {
    statusBadgeSettings.status.value = 'verified';
    statusBadgeSettings.showIcon.value = false;
    statusBadgeSettings.appearance.value = 'plain';
    statusBadgeSettings.compact.value = true;
    resetStatusBadgeDemo();
    expect({ status: statusBadgeSettings.status.value, showIcon: statusBadgeSettings.showIcon.value, appearance: statusBadgeSettings.appearance.value, compact: statusBadgeSettings.compact.value }).toEqual({ status: 'started', showIcon: true, appearance: 'filled', compact: false });
  });

  it('resets every canonical TicketRow demo setting', () => {
    ticketRowSettings.title.value = 'Changed';
    ticketRowSettings.status.value = 'verified';
    ticketRowSettings.priority.value = 'urgent';
    ticketRowSettings.category.value = 'bug';
    ticketRowSettings.tags.value = 'one';
    ticketRowSettings.upNext.value = false;
    ticketRowSettings.blocked.value = true;
    ticketRowSettings.needsReview.value = true;
    ticketRowSettings.selected.value = true;
    ticketRowSettings.busy.value = false;
    ticketRowSettings.categoryIcon.value = 'bug';
    ticketRowSettings.categoryColor.value = '#ef4444';
    ticketRowSettings.agentName.value = 'Codex';
    ticketRowSettings.updatedLabel.value = 'Now';
    ticketRowSettings.event.value = 'Changed';
    resetTicketRowDemo();
    expect({
      title: ticketRowSettings.title.value, status: ticketRowSettings.status.value,
      priority: ticketRowSettings.priority.value, category: ticketRowSettings.category.value,
      tags: ticketRowSettings.tags.value, upNext: ticketRowSettings.upNext.value,
      blocked: ticketRowSettings.blocked.value, needsReview: ticketRowSettings.needsReview.value,
      selected: ticketRowSettings.selected.value, busy: ticketRowSettings.busy.value,
      categoryIcon: ticketRowSettings.categoryIcon.value, categoryColor: ticketRowSettings.categoryColor.value,
      agentName: ticketRowSettings.agentName.value, updatedLabel: ticketRowSettings.updatedLabel.value,
      event: ticketRowSettings.event.value,
    }).toEqual({
      title: 'Build the first client ticket list', status: 'started', priority: 'high',
      category: 'feature', tags: 'client, ux', upNext: true, selected: false,
      blocked: false, needsReview: false, busy: true, categoryIcon: 'sparkles',
      categoryColor: '#3b82f6', event: 'No actions yet',
      agentName: 'Claude', updatedLabel: '1h ago',
    });
  });
});
