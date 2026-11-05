import { Button } from '@kerfjs/ui/button';
import { CatalogExample, CatalogExampleStack } from '@kerfjs/ui/catalog';
import { Select } from '@kerfjs/ui/select';
import { delegate, signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import { StatsRightNow } from '../components/stats-right-now';
import { rightNowRows, type RightNowTargets, rightNowTiles } from '../stats-right-now';
import type { DemoModule, FormControl } from './demo-module';
import { DemoSettingsForm } from './demo-settings-form';
import { GLANCE_FIXTURE_NOW, glanceScenario } from './live-state-fixtures';
import { syncSettingsControls } from './settings-controls';

type Scope = 'all' | 'project';
type Scenario = 'mixed' | 'calm' | 'empty';

const DEFAULTS = { scope: 'all' as Scope, scenario: 'mixed' as Scenario };

export const statsRightNowSettings = {
  scope: signal<Scope>(DEFAULTS.scope),
  scenario: signal<Scenario>(DEFAULTS.scenario),
};

export function resetStatsRightNowDemo(root?: ParentNode): void {
  statsRightNowSettings.scope.value = DEFAULTS.scope;
  statsRightNowSettings.scenario.value = DEFAULTS.scenario;
  if (root) syncSettingsControls(root, 'stats-right-now', { values: { ...DEFAULTS } });
}

/** Demo projects are not open, so actions route to fixed synthetic targets. */
const TARGETS: RightNowTargets = {
  projectFor: (state) => state.checkouts[0],
  terminalFor: (projectId) => `${projectId}:demo-terminal`,
};

function scenarioStates() {
  const states = glanceScenario();
  const scenario = statsRightNowSettings.scenario.value;
  const inScope =
    statsRightNowSettings.scope.value === 'project' ? states.filter((state) => state.name === 'Hot Sheet 2') : states;
  if (scenario === 'empty') return [];
  if (scenario === 'calm') return inScope.filter((state) => state.state === 'active' || state.state === 'stopped');
  return inScope;
}

/** The Right now section with the mock's four synthetic projects (HS2-ERTNJ7). */
export function StatsRightNowDemo() {
  const states = scenarioStates();
  const now = GLANCE_FIXTURE_NOW;
  return (
    <CatalogExampleStack>
      <CatalogExample
        label="Right Now"
        note="Sorted by need; only Needs you and problem rows take colour. Open terminal and Answer route through the app's delegated actions."
        viewport={{ width: 'full' }}
      >
        {StatsRightNow({
          scope:
            statsRightNowSettings.scope.value === 'project'
              ? { kind: 'project', name: 'Hot Sheet 2' }
              : { kind: 'all' },
          stores: states.length,
          tiles: rightNowTiles(states, now),
          rows: rightNowRows(states, now, TARGETS),
          completed: { count: 9, sinceMs: Date.parse('2026-11-06T00:00:00Z') },
          asOfMs: now,
        })}
      </CatalogExample>
    </CatalogExampleStack>
  );
}

export function StatsRightNowSettings() {
  return (
    <DemoSettingsForm settings="stats-right-now">
      <Select
        name="scope"
        label="Scope"
        value={statsRightNowSettings.scope.value}
        choices={[
          { value: 'all', label: 'All Projects' },
          { value: 'project', label: 'One Project (Hot Sheet 2)' },
        ]}
      />
      <Select
        name="scenario"
        label="Scenario"
        value={statsRightNowSettings.scenario.value}
        choices={[
          { value: 'mixed', label: 'Stuck, Waiting, Active, Stopped' },
          { value: 'calm', label: 'All Normal' },
          { value: 'empty', label: 'No Live Projects' },
        ]}
      />
      <Button type="button" action={'reset-settings'}>
        Reset
      </Button>
    </DemoSettingsForm>
  );
}

function wireStatsRightNowDemo(root: HTMLElement): () => void {
  const listeners = createScope();
  listeners.add(
    delegate(root, 'change', '[data-settings="stats-right-now"] [name]', (_event, target) => {
      const control = target as FormControl;
      const name = control.getAttribute('name');
      if (name === 'scope') statsRightNowSettings.scope.value = control.value as Scope;
      if (name === 'scenario') statsRightNowSettings.scenario.value = control.value as Scenario;
    }),
  );
  return () => {
    listeners.dispose();
  };
}

export const demoModule: DemoModule = {
  entries: {
    'stats-right-now': {
      catalogExamples: true,
      stackWidth: 'full',
      Demo: StatsRightNowDemo,
      Settings: StatsRightNowSettings,
      reset: resetStatsRightNowDemo,
    },
  },
  wire: wireStatsRightNowDemo,
};
