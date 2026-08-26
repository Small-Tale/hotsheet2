import { signal } from 'kerfjs';
import { StatusBadge, type StatusBadgeAppearance, type TicketStatus } from '../components/status-badge';
import { syncSettingsControls } from './settings-controls';

export const statusBadgeSettings = {
  status: signal<TicketStatus>('started'),
  showIcon: signal(true),
  appearance: signal<StatusBadgeAppearance>('filled'),
  compact: signal(false),
};

export function resetStatusBadgeDemo(root?: ParentNode): void {
  statusBadgeSettings.status.value = 'started';
  statusBadgeSettings.showIcon.value = true;
  statusBadgeSettings.appearance.value = 'filled';
  statusBadgeSettings.compact.value = false;
  if (root) syncSettingsControls(root, 'status-badge', {
    values: { status: statusBadgeSettings.status.value, appearance: statusBadgeSettings.appearance.value },
    checked: { 'show-icon': statusBadgeSettings.showIcon.value, compact: statusBadgeSettings.compact.value },
  });
}

export function StatusBadgeDemo() {
  return <section class="component-stage" aria-label="StatusBadge demo"><div class="component-stage__canvas">{StatusBadge({ status: statusBadgeSettings.status.value, showIcon: statusBadgeSettings.showIcon.value, appearance: statusBadgeSettings.appearance.value, compact: statusBadgeSettings.compact.value })}</div><p class="component-stage__guidance">Status is always communicated with text; its icon is reinforcing decoration.</p></section>;
}

export function StatusBadgeSettings() {
  return <form class="settings-form" data-settings="status-badge">
    <wa-select name="status" label="Status" value={statusBadgeSettings.status.value}>{(['not_started', 'started', 'completed', 'verified', 'backlog'] as const).map(value => <wa-option value={value}>{value.replace('_', ' ')}</wa-option>)}</wa-select>
    <wa-select name="appearance" label="Appearance" value={statusBadgeSettings.appearance.value}><wa-option value="filled">Filled</wa-option><wa-option value="plain">Plain</wa-option></wa-select>
    <wa-checkbox name="show-icon" checked={statusBadgeSettings.showIcon.value}>Show icon</wa-checkbox>
    <wa-checkbox name="compact" checked={statusBadgeSettings.compact.value}>Compact</wa-checkbox>
    <wa-button type="button" data-action="reset-settings">Reset</wa-button>
  </form>;
}
