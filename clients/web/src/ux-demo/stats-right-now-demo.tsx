import { Button } from '@kerfjs/ui/button';
import { CatalogExample, CatalogExampleStack } from '@kerfjs/ui/catalog';
import { Select } from '@kerfjs/ui/select';
import { delegate, signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import type { LiveStoreState } from '../api';
import { StatsRightNow } from '../components/stats-right-now';
import { rightNowRows, type RightNowTargets, rightNowTiles } from '../stats-right-now';
import type { DemoModule, FormControl } from './demo-module';
import { DemoSettingsForm } from './demo-settings-form';
import { GLANCE_FIXTURE_NOW, glanceScenario } from './live-state-fixtures';
import { syncSettingsControls } from './settings-controls';

type Scope = 'all' | 'project';
type Scenario = 'mixed' | 'calm' | 'empty';
/** The `main` CI run the first project reported (HS2-CC7TCB adds a stale running report). */
type CiScenario = 'none' | 'running' | 'failed' | 'stale';

const DEFAULTS = { scope: 'all' as Scope, scenario: 'mixed' as Scenario, ci: 'none' as CiScenario };

export const statsRightNowSettings = {
  scope: signal<Scope>(DEFAULTS.scope),
  scenario: signal<Scenario>(DEFAULTS.scenario),
  ci: signal<CiScenario>(DEFAULTS.ci),
};

export function resetStatsRightNowDemo(root?: ParentNode): void {
  statsRightNowSettings.scope.value = DEFAULTS.scope;
  statsRightNowSettings.scenario.value = DEFAULTS.scenario;
  statsRightNowSettings.ci.value = DEFAULTS.ci;
  if (root) syncSettingsControls(root, 'stats-right-now', { values: { ...DEFAULTS } });
}

/** Demo projects are not open, so actions route to fixed synthetic targets. */
const TARGETS: RightNowTargets = {
  projectFor: (state) => state.checkouts[0],
  terminalFor: (projectId) => `${projectId}:demo-terminal`,
};

const MINUTE = 60_000;

/**
 * A synthetic `main` run at the fixture clock: running 12 minutes, failed 3 minutes ago, or reported running
 * 2 h 39 m ago and never again (HS2-CC7TCB).
 */
function demoCi(scenario: CiScenario, now: number): LiveStoreState['ci'] {
  if (scenario === 'none') return undefined;
  const stale = scenario === 'stale',
    failed = scenario === 'failed';
  return {
    status: failed ? 'failure' : 'in_progress',
    checkout: 'hotsheet',
    branch: 'main',
    sha: '8f6c8d81f0a1',
    run_url: null,
    workflow: 'CI',
    started_at_ms: now - (stale ? 165 : failed ? 40 : 12) * MINUTE,
    finished_at_ms: failed ? now - 3 * MINUTE : null,
    reported_at_ms: now - (stale ? 159 : failed ? 3 : 1) * MINUTE,
  };
}

function scenarioStates() {
  const ci = demoCi(statsRightNowSettings.ci.value, GLANCE_FIXTURE_NOW);
  const states = glanceScenario().map((state, index) => (index === 0 && ci ? { ...state, ci } : state));
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
      <Select
        name="ci"
        label="Main CI"
        value={statsRightNowSettings.ci.value}
        choices={[
          { value: 'none', label: 'None Reported' },
          { value: 'running', label: 'Running' },
          { value: 'failed', label: 'Failed' },
          { value: 'stale', label: 'Running, Reported Hours Ago' },
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
      if (name === 'ci') statsRightNowSettings.ci.value = control.value as CiScenario;
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
