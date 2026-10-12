import { Select } from '@kerfjs/ui/select';
import { delegate, signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import type { StartedPhase } from '../api';
import { StatusBadge, type StatusBadgeAppearance, type TicketStatus } from '../components/status-badge';
import type { DemoModule, FormControl } from './demo-module';
import { syncSettingsControls } from './settings-controls';

export const statusBadgeSettings = {
  status: signal<TicketStatus>('started'),
  startedPhase: signal<'' | StartedPhase>(''),
  showIcon: signal(true),
  appearance: signal<StatusBadgeAppearance>('filled'),
  compact: signal(false),
};

export function resetStatusBadgeDemo(root?: ParentNode): void {
  statusBadgeSettings.status.value = 'started';
  statusBadgeSettings.startedPhase.value = '';
  statusBadgeSettings.showIcon.value = true;
  statusBadgeSettings.appearance.value = 'filled';
  statusBadgeSettings.compact.value = false;
  if (root)
    syncSettingsControls(root, 'status-badge', {
      values: {
        status: statusBadgeSettings.status.value,
        'started-phase': statusBadgeSettings.startedPhase.value,
        appearance: statusBadgeSettings.appearance.value,
      },
      checked: { 'show-icon': statusBadgeSettings.showIcon.value, compact: statusBadgeSettings.compact.value },
    });
}

export function StatusBadgeDemo() {
  return (
    <section class="component-stage" aria-label="StatusBadge demo">
      <div class="component-stage__canvas">
        {StatusBadge({
          status: statusBadgeSettings.status.value,
          startedPhase: statusBadgeSettings.startedPhase.value || undefined,
          showIcon: statusBadgeSettings.showIcon.value,
          appearance: statusBadgeSettings.appearance.value,
          compact: statusBadgeSettings.compact.value,
        })}
      </div>
      <p class="component-stage__guidance">
        Status is always communicated with text; its icon is reinforcing decoration.
      </p>
    </section>
  );
}

export function StatusBadgeSettings() {
  return (
    <form class="settings-form" data-settings="status-badge">
      <Select
        name="status"
        label="Status"
        value={statusBadgeSettings.status.value}
        choices={['not_started', 'started', 'completed', 'verified', 'backlog', 'archive'].map((value) => ({
          value,
          label: value.replace('_', ' '),
        }))}
      />
      <Select
        name="started-phase"
        label="Started phase"
        value={statusBadgeSettings.startedPhase.value}
        choices={[
          { value: '', label: 'None' },
          { value: 'analyzing', label: 'Analyzing' },
          { value: 'planning', label: 'Planning' },
          { value: 'working', label: 'Working' },
          { value: 'initial_testing', label: 'Initial testing' },
          { value: 'integrating', label: 'Integrating' },
          { value: 'final_testing', label: 'Final testing' },
        ]}
      />
      <Select
        name="appearance"
        label="Appearance"
        value={statusBadgeSettings.appearance.value}
        choices={[
          { value: 'filled', label: 'Filled' },
          { value: 'plain', label: 'Plain' },
        ]}
      />
      <wa-checkbox name="show-icon" checked={statusBadgeSettings.showIcon.value}>
        Show icon
      </wa-checkbox>
      <wa-checkbox name="compact" checked={statusBadgeSettings.compact.value}>
        Compact
      </wa-checkbox>
      <wa-button type="button" data-action="reset-settings">
        Reset
      </wa-button>
    </form>
  );
}

/** Install this file's demo interactions on the catalog root (HS2-HMVBZW). */
function wireStatusBadgeDemo(root: HTMLElement): () => void {
  const demoListeners = createScope();
  demoListeners.add(
    delegate(root, 'change', '[data-settings="status-badge"] [name]', (_event, target) => {
      const control = target as FormControl;
      if (control.getAttribute('name') === 'status')
        statusBadgeSettings.status.value = control.value as typeof statusBadgeSettings.status.value;
      if (control.getAttribute('name') === 'started-phase')
        statusBadgeSettings.startedPhase.value = control.value as typeof statusBadgeSettings.startedPhase.value;
      if (control.getAttribute('name') === 'appearance')
        statusBadgeSettings.appearance.value = control.value as typeof statusBadgeSettings.appearance.value;
      if (control.getAttribute('name') === 'show-icon') statusBadgeSettings.showIcon.value = control.checked;
      if (control.getAttribute('name') === 'compact') statusBadgeSettings.compact.value = control.checked;
    }),
  );
  return () => {
    demoListeners.dispose();
  };
}

/** The catalog entries this file renders (HS2-HMVBZW). */
export const demoModule: DemoModule = {
  entries: {
    'status-badge': { Demo: StatusBadgeDemo, Settings: StatusBadgeSettings, reset: resetStatusBadgeDemo },
  },
  wire: wireStatusBadgeDemo,
};
