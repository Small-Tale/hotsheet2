import { Button } from '@kerfjs/ui/button';
import { CatalogExample, CatalogExampleStack } from '@kerfjs/ui/catalog';
import { Select } from '@kerfjs/ui/select';
import { delegate, signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import { SidebarPane } from '../components/sidebar-panel';
import { systemsInspectorPanel } from '../components/systems-inspector';
import { systemsSidebarPanel, type SystemsToolCapacityState } from '../components/systems-sidebar';
import { SystemsView } from '../components/systems-view';
import type { DemoModule, FormControl } from './demo-module';
import { DemoSettingsForm } from './demo-settings-form';
import { syncSettingsControls } from './settings-controls';
import {
  SYSTEMS_DEMO_MACHINES,
  systemsDemoInspector,
  type SystemsDemoMachine,
  type SystemsDemoScenario,
  systemsDemoSidebar,
  systemsDemoView,
} from './systems-fixtures';

type ToolCapacityChoice = 'ready' | 'loading' | 'none' | 'error';
const DEFAULTS = {
  machine: 'local' as SystemsDemoMachine,
  scenario: 'ready' as SystemsDemoScenario,
  toolCapacity: 'ready' as ToolCapacityChoice,
};

export const systemsDemoSettings = {
  machine: signal<SystemsDemoMachine>(DEFAULTS.machine),
  scenario: signal<SystemsDemoScenario>(DEFAULTS.scenario),
  toolCapacity: signal<ToolCapacityChoice>(DEFAULTS.toolCapacity),
};

export function resetSystemsDemo(root?: ParentNode): void {
  systemsDemoSettings.machine.value = DEFAULTS.machine;
  systemsDemoSettings.scenario.value = DEFAULTS.scenario;
  systemsDemoSettings.toolCapacity.value = DEFAULTS.toolCapacity;
  if (root) syncSettingsControls(root, 'systems', { values: { ...DEFAULTS } });
}

function toolCapacityState(choice: ToolCapacityChoice): SystemsToolCapacityState | undefined {
  if (choice === 'loading') return { status: 'loading' };
  if (choice === 'none') return { status: 'none' };
  if (choice === 'error') return { status: 'error', message: 'The Hot Sheet server could not be reached.' };
  return undefined;
}

/** The Systems workspace for each machine and reading state (HS2-DR0ZSQ). */
export function SystemsViewDemo() {
  return (
    <CatalogExampleStack>
      <CatalogExample
        label="Machine Detail"
        note="The selected machine's This Mac cards, AI tools, capabilities and toolchains. A machine without a connected project says how to connect it."
        viewport={{ width: 'full' }}
      >
        {SystemsView(systemsDemoView(systemsDemoSettings.machine.value, systemsDemoSettings.scenario.value))}
      </CatalogExample>
    </CatalogExampleStack>
  );
}

/** The Machines list and AI Tool Capacity rail. */
export function SystemsSidebarDemo() {
  return (
    <CatalogExample label="Machines" viewport={{ width: 'sidebar' }}>
      <SidebarPane
        parts={systemsSidebarPanel(
          systemsDemoSidebar(
            systemsDemoSettings.machine.value,
            systemsDemoSettings.scenario.value,
            toolCapacityState(systemsDemoSettings.toolCapacity.value),
          ),
        )}
      />
    </CatalogExample>
  );
}

/** The selected machine's AI sessions rail. */
export function SystemsInspectorDemo() {
  return (
    <CatalogExample label="Machine Details" viewport={{ width: 'sidebar' }}>
      <SidebarPane
        parts={systemsInspectorPanel(
          systemsDemoInspector(systemsDemoSettings.machine.value, systemsDemoSettings.scenario.value),
        )}
      />
    </CatalogExample>
  );
}

export function SystemsSettings() {
  return (
    <DemoSettingsForm settings="systems">
      <Select
        name="machine"
        label="Machine"
        value={systemsDemoSettings.machine.value}
        choices={[
          { value: 'local', label: 'This Computer' },
          { value: 'build', label: 'Paired Remote, Busy' },
          { value: 'offline', label: 'Paired Remote, Not Connected' },
          { value: 'unreachable', label: 'Paired Remote, Not Reachable' },
          { value: 'revoked', label: 'Paired Remote, Revoked' },
        ]}
      />
      <Select
        name="scenario"
        label="Readings"
        value={systemsDemoSettings.scenario.value}
        choices={[
          { value: 'ready', label: 'Read' },
          { value: 'loading', label: 'Loading' },
          { value: 'failed', label: 'First Read Failed' },
          { value: 'stale', label: 'Stale After a Failure' },
        ]}
      />
      <Select
        name="toolCapacity"
        label="AI Tool Capacity"
        value={systemsDemoSettings.toolCapacity.value}
        choices={[
          { value: 'ready', label: 'Read' },
          { value: 'loading', label: 'Loading' },
          { value: 'none', label: 'No Orchestration Project' },
          { value: 'error', label: 'Failed' },
        ]}
      />
      <Button type="button" action={'reset-settings'}>
        Reset
      </Button>
    </DemoSettingsForm>
  );
}

function wireSystemsDemo(root: HTMLElement): () => void {
  const listeners = createScope();
  listeners.add(
    delegate(root, 'change', '[data-settings="systems"] [name]', (_event, target) => {
      const control = target as FormControl;
      const name = control.getAttribute('name');
      if (name === 'machine') systemsDemoSettings.machine.value = control.value as SystemsDemoMachine;
      if (name === 'scenario') systemsDemoSettings.scenario.value = control.value as SystemsDemoScenario;
      if (name === 'toolCapacity') systemsDemoSettings.toolCapacity.value = control.value as ToolCapacityChoice;
    }),
  );
  // A Machines row selects its machine, as the app's does.
  listeners.add(
    delegate(root, 'click', '[data-action="select-systems-machine"][data-machine-id]', (_event, target) => {
      const id = (target as HTMLElement).dataset.machineId;
      const machine = (Object.keys(SYSTEMS_DEMO_MACHINES) as SystemsDemoMachine[]).find(
        (key) => SYSTEMS_DEMO_MACHINES[key].id === id,
      );
      if (machine) systemsDemoSettings.machine.value = machine;
    }),
  );
  return () => {
    listeners.dispose();
  };
}

const settings = { Settings: SystemsSettings, reset: resetSystemsDemo };

export const demoModule: DemoModule = {
  entries: {
    'systems-view': { catalogExamples: true, stackWidth: 'full', Demo: SystemsViewDemo, ...settings },
    'systems-sidebar': { catalogExamples: true, Demo: SystemsSidebarDemo, ...settings },
    'systems-inspector': { catalogExamples: true, Demo: SystemsInspectorDemo, ...settings },
  },
  wire: wireSystemsDemo,
};
