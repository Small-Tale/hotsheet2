import { signal } from 'kerfjs';

import { CATEGORY_COLORS, CATEGORY_ICONS } from '../components/category-presentation';
import type { TicketStatus } from '../components/status-badge';
import { type TicketPriority,TicketRow } from '../components/ticket-row';
import { syncSettingsControls } from './settings-controls';

export const ticketRowSettings = {
  title: signal('Build the first client ticket list'),
  status: signal<TicketStatus>('started'),
  priority: signal<TicketPriority>('high'),
  category: signal('feature'),
  tags: signal('client, ux'),
  upNext: signal(true),
  blocked: signal(false),
  needsReview: signal(false),
  selected: signal(false),
  busy: signal(true),
  categoryIcon: signal('sparkles'),
  categoryColor: signal('#3b82f6'),
  agentName: signal('Claude'),
  updatedLabel: signal('1h ago'),
  event: signal('No actions yet'),
};

export function resetTicketRowDemo(root?: ParentNode): void {
  ticketRowSettings.title.value = 'Build the first client ticket list';
  ticketRowSettings.status.value = 'started';
  ticketRowSettings.priority.value = 'high';
  ticketRowSettings.category.value = 'feature';
  ticketRowSettings.tags.value = 'client, ux';
  ticketRowSettings.upNext.value = true;
  ticketRowSettings.blocked.value = false;
  ticketRowSettings.needsReview.value = false;
  ticketRowSettings.selected.value = false;
  ticketRowSettings.busy.value = true;
  ticketRowSettings.categoryIcon.value = 'sparkles';
  ticketRowSettings.categoryColor.value = '#3b82f6';
  ticketRowSettings.agentName.value = 'Claude';
  ticketRowSettings.updatedLabel.value = '1h ago';
  ticketRowSettings.event.value = 'No actions yet';
  if (root) syncSettingsControls(root, 'ticket-list-row', {
    values: { title: ticketRowSettings.title.value, status: ticketRowSettings.status.value, priority: ticketRowSettings.priority.value, category: ticketRowSettings.category.value, tags: ticketRowSettings.tags.value, 'category-icon': ticketRowSettings.categoryIcon.value, 'category-color': ticketRowSettings.categoryColor.value, agent: ticketRowSettings.agentName.value, updated: ticketRowSettings.updatedLabel.value },
    checked: { 'up-next': ticketRowSettings.upNext.value, blocked: ticketRowSettings.blocked.value, 'needs-review': ticketRowSettings.needsReview.value, selected: ticketRowSettings.selected.value, busy: ticketRowSettings.busy.value },
  });
}

export function TicketRowDemo() {
  return <section class="component-stage component-stage--row" aria-label="TicketRow demo">
    <div class="component-stage__canvas component-stage__canvas--row" role="listbox" aria-label="Example ticket list">
      {TicketRow({ slug: 'HS2-D3M0', title: ticketRowSettings.title.value, status: ticketRowSettings.status.value, priority: ticketRowSettings.priority.value, category: ticketRowSettings.category.value, tags: ticketRowSettings.tags.value.split(','), upNext: ticketRowSettings.upNext.value, blocked: ticketRowSettings.blocked.value, needsReview: ticketRowSettings.needsReview.value, selected: ticketRowSettings.selected.value, busy: ticketRowSettings.busy.value, categoryIcon: ticketRowSettings.categoryIcon.value, categoryColor: ticketRowSettings.categoryColor.value, agentName: ticketRowSettings.agentName.value, updatedLabel: ticketRowSettings.updatedLabel.value })}
    </div>
    <p class="component-stage__event" aria-live="polite">{ticketRowSettings.event}</p>
    <p class="component-stage__guidance">The full row is the selection target. Metadata stays scannable while tags and transient AI activity remain secondary.</p>
  </section>;
}

export function TicketRowSettings() {
  return <form class="settings-form" data-settings="ticket-list-row">
    <wa-input name="title" label="Title" value={ticketRowSettings.title.value}></wa-input>
    <wa-select name="status" label="Status" value={ticketRowSettings.status.value}>{(['not_started', 'started', 'completed', 'verified', 'backlog'] as const).map(value => <wa-option value={value}>{value.replace('_', ' ')}</wa-option>)}</wa-select>
    <wa-select name="priority" label="Priority" value={ticketRowSettings.priority.value}>{(['low', 'default', 'high', 'urgent'] as const).map(value => <wa-option value={value}>{value}</wa-option>)}</wa-select>
    <wa-input name="category" label="Category" value={ticketRowSettings.category.value}></wa-input>
    <wa-select name="category-icon" label="Category icon" value={ticketRowSettings.categoryIcon.value}>{CATEGORY_ICONS.map(option => <wa-option value={option.value}>{option.label}</wa-option>)}</wa-select>
    <wa-select name="category-color" label="Category icon color" value={ticketRowSettings.categoryColor.value}>{CATEGORY_COLORS.map(option => <wa-option value={option.value}>{option.label}</wa-option>)}</wa-select>
    <wa-input name="tags" label="Tags (comma separated)" value={ticketRowSettings.tags.value}></wa-input>
    <wa-input name="agent" label="Active agent" value={ticketRowSettings.agentName.value}></wa-input>
    <wa-input name="updated" label="Updated label" value={ticketRowSettings.updatedLabel.value}></wa-input>
    <wa-checkbox name="up-next" checked={ticketRowSettings.upNext.value}>Up Next</wa-checkbox>
    <wa-checkbox name="blocked" checked={ticketRowSettings.blocked.value}>Blocked</wa-checkbox>
    <wa-checkbox name="needs-review" checked={ticketRowSettings.needsReview.value}>Needs review</wa-checkbox>
    <wa-checkbox name="selected" checked={ticketRowSettings.selected.value}>Selected</wa-checkbox>
    <wa-checkbox name="busy" checked={ticketRowSettings.busy.value}>AI working</wa-checkbox>
    <wa-button type="button" data-action="reset-settings">Reset</wa-button>
  </form>;
}
