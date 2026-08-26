import { signal } from 'kerfjs';
import { StatusBadge, type TicketStatus } from '../components/status-badge';
import { syncSettingsControls } from './settings-controls';

export const statusBadgeSettings = {
  status: signal<TicketStatus>('started'),
  showIcon: signal(true),
};

export function resetStatusBadgeDemo(root?: ParentNode): void {
  statusBadgeSettings.status.value = 'started';
  statusBadgeSettings.showIcon.value = true;
  if (root) syncSettingsControls(root, 'status-badge', { values: { status: statusBadgeSettings.status.value }, checked: { 'show-icon': statusBadgeSettings.showIcon.value } });
}

export function StatusBadgeDemo() {
  return <section class="component-stage" aria-label="StatusBadge demo"><div class="component-stage__canvas">{StatusBadge({ status: statusBadgeSettings.status.value, showIcon: statusBadgeSettings.showIcon.value })}</div><p class="component-stage__guidance">Status is always communicated with text; its icon is reinforcing decoration.</p></section>;
}

export function StatusBadgeSettings() {
  return <form class="settings-form" data-settings="status-badge">
    <wa-select name="status" label="Status" value={statusBadgeSettings.status.value}>{(['not_started', 'started', 'completed', 'verified', 'backlog'] as const).map(value => <wa-option value={value}>{value.replace('_', ' ')}</wa-option>)}</wa-select>
    <wa-checkbox name="show-icon" checked={statusBadgeSettings.showIcon.value}>Show icon</wa-checkbox>
    <wa-button type="button" data-action="reset-settings">Reset</wa-button>
  </form>;
}
