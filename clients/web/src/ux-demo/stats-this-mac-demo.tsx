import { Button } from '@kerfjs/ui/button';
import { CatalogExample, CatalogExampleStack } from '@kerfjs/ui/catalog';
import { Select } from '@kerfjs/ui/select';
import { delegate, signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import { StatsThisMac } from '../components/stats-this-mac';
import type { HostHealthResourceState } from '../host-health-resource';
import type { DemoModule, FormControl } from './demo-module';
import { DemoSettingsForm } from './demo-settings-form';
import { hostHealthFixture, type HostHealthScenario } from './host-health-fixtures';
import { syncSettingsControls } from './settings-controls';

type Scenario = HostHealthScenario | 'loading' | 'failed' | 'stale';

const DEFAULTS = { scenario: 'normal' as Scenario };

export const statsThisMacSettings = { scenario: signal<Scenario>(DEFAULTS.scenario) };

export function resetStatsThisMacDemo(root?: ParentNode): void {
  statsThisMacSettings.scenario.value = DEFAULTS.scenario;
  if (root) syncSettingsControls(root, 'stats-this-mac', { values: { ...DEFAULTS } });
}

function demoState(scenario: Scenario): HostHealthResourceState {
  if (scenario === 'loading') return { loading: true };
  if (scenario === 'failed') return { loading: false, error: 'The server did not answer.' };
  if (scenario === 'stale')
    return { snapshot: hostHealthFixture('normal'), loading: false, error: 'The server did not answer.' };
  return { snapshot: hostHealthFixture(scenario), loading: false };
}

/** The server host's cards for each health scenario (HS2-NS0AYR). */
export function StatsThisMacDemo() {
  return (
    <CatalogExampleStack>
      <CatalogExample
        label="This Mac"
        note="Cards tint only after a threshold is crossed; network and thermal appear only while degraded. Unsupported signals say so."
        viewport={{ width: 'full' }}
      >
        {StatsThisMac({ state: demoState(statsThisMacSettings.scenario.value) })}
      </CatalogExample>
    </CatalogExampleStack>
  );
}

export function StatsThisMacSettings() {
  return (
    <DemoSettingsForm settings="stats-this-mac">
      <Select
        name="scenario"
        label="Scenario"
        value={statsThisMacSettings.scenario.value}
        choices={[
          { value: 'normal', label: 'All Normal' },
          { value: 'crossed', label: 'Disk, CPU, and Network Crossed' },
          { value: 'unsupported', label: 'Unsupported Platform' },
          { value: 'disks', label: 'Several Disks and a New One' },
          { value: 'loading', label: 'Loading' },
          { value: 'failed', label: 'First Read Failed' },
          { value: 'stale', label: 'Stale Snapshot' },
        ]}
      />
      <Button type="button" action={'reset-settings'}>
        Reset
      </Button>
    </DemoSettingsForm>
  );
}

function wireStatsThisMacDemo(root: HTMLElement): () => void {
  const listeners = createScope();
  listeners.add(
    delegate(root, 'change', '[data-settings="stats-this-mac"] [name]', (_event, target) => {
      const control = target as FormControl;
      if (control.getAttribute('name') === 'scenario') statsThisMacSettings.scenario.value = control.value as Scenario;
    }),
  );
  return () => {
    listeners.dispose();
  };
}

export const demoModule: DemoModule = {
  entries: {
    'stats-this-mac': {
      catalogExamples: true,
      stackWidth: 'full',
      Demo: StatsThisMacDemo,
      Settings: StatsThisMacSettings,
      reset: resetStatsThisMacDemo,
    },
  },
  wire: wireStatsThisMacDemo,
};
