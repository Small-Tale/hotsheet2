import { Button } from '@kerfjs/ui/button';
import { CatalogExample, CatalogExampleStack } from '@kerfjs/ui/catalog';
import { Select, type SelectChoice } from '@kerfjs/ui/select';
import { delegate, signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import { ProjectLiveSection } from '../components/project-live-section';
import { COMMANDS_AND_AI_ACTIONS } from '../interaction-attrs/commands-and-ai';
import { hostCallouts, projectLiveSummary } from '../project-live-summary';
import type { DemoModule, FormControl } from './demo-module';
import { DemoSettingsForm } from './demo-settings-form';
import {
  DEMO_LIVE_NOW_MS,
  demoHostHealth,
  type DemoHostScenario,
  type DemoLiveScenario,
  demoLiveState,
} from './live-section-fixtures';
import { syncSettingsControls } from './settings-controls';

const SCENARIO_CHOICES: SelectChoice[] = [
  { value: 'quiet', label: 'Active, Nothing Needed' },
  { value: 'attention', label: 'Needs You and Past ETA' },
  { value: 'may_be_stuck', label: 'May Be Stuck' },
  { value: 'stalled', label: 'Stalled' },
  { value: 'halted', label: 'Halted' },
  { value: 'stopped', label: 'Stopped' },
  { value: 'none', label: 'No Live State Yet' },
];
const HOST_CHOICES: SelectChoice[] = [
  { value: 'normal', label: 'All Normal' },
  { value: 'disk-warning', label: 'Disk Filling (Warning)' },
  { value: 'disk-critical', label: 'Disk Almost Full (Critical)' },
  { value: 'battery', label: 'On Battery (Warning)' },
];

const MODE_CHOICES: SelectChoice[] = [
  { value: 'compact', label: 'Compact (Default)' },
  { value: 'expanded', label: 'Expanded Details' },
];

const DEFAULTS = { scenario: 'attention', host: 'disk-warning', mode: 'compact' } as const;

export const projectLiveSectionSettings = {
  scenario: signal<string>(DEFAULTS.scenario),
  host: signal<string>(DEFAULTS.host),
  mode: signal<string>(DEFAULTS.mode),
};

export function resetProjectLiveSectionDemo(root?: ParentNode): void {
  projectLiveSectionSettings.scenario.value = DEFAULTS.scenario;
  projectLiveSectionSettings.host.value = DEFAULTS.host;
  projectLiveSectionSettings.mode.value = DEFAULTS.mode;
  if (root) syncSettingsControls(root, 'project-live-section', { values: { ...DEFAULTS } });
}

function section(scenario: string, host: string, expanded: boolean) {
  return (
    <ProjectLiveSection
      summary={
        scenario === 'none'
          ? undefined
          : projectLiveSummary([demoLiveState(scenario as DemoLiveScenario)], DEMO_LIVE_NOW_MS)
      }
      callouts={hostCallouts(demoHostHealth(host as DemoHostScenario))}
      expanded={expanded}
    />
  );
}

/** The interactive section; Settings switch the live state, host health, and mode. */
export function ProjectLiveSectionDemo() {
  const { scenario, host, mode } = projectLiveSectionSettings;
  return (
    <CatalogExampleStack>
      <CatalogExample
        label="Interactive"
        note="The header action switches between the compact line and the detail rows. Hover a gauge or chip for its explanation, which is also its accessible name."
      >
        <div class="demo-sidebar-width">{section(scenario.value, host.value, mode.value === 'expanded')}</div>
      </CatalogExample>
    </CatalogExampleStack>
  );
}

export function ProjectLiveSectionSettings() {
  const { scenario, host, mode } = projectLiveSectionSettings;
  return (
    <DemoSettingsForm settings="project-live-section">
      <Select name="scenario" label="Live State" value={scenario.value} choices={SCENARIO_CHOICES} />
      <Select name="host" label="Host Health" value={host.value} choices={HOST_CHOICES} />
      <Select name="mode" label="Mode" value={mode.value} choices={MODE_CHOICES} />
      <Button type="button" action={'reset-settings'}>
        Reset
      </Button>
    </DemoSettingsForm>
  );
}

/** Install this file's demo interactions on the catalog root. */
function wireProjectLiveSectionDemo(root: HTMLElement): () => void {
  const demoListeners = createScope();
  demoListeners.add(
    delegate(root, 'change', '[data-settings="project-live-section"] [name]', (_event, target) => {
      const control = target as FormControl;
      const name = control.getAttribute('name');
      if (name === 'scenario') projectLiveSectionSettings.scenario.value = control.value;
      if (name === 'host') projectLiveSectionSettings.host.value = control.value;
      if (name === 'mode') projectLiveSectionSettings.mode.value = control.value;
    }),
  );
  demoListeners.add(
    delegate(root, 'click', COMMANDS_AND_AI_ACTIONS.toggleLiveSection.selector, (_event, target) => {
      if (!target.closest('.demo-sidebar-width')) return;
      const next = projectLiveSectionSettings.mode.value === 'expanded' ? 'compact' : 'expanded';
      projectLiveSectionSettings.mode.value = next;
      syncSettingsControls(root, 'project-live-section', {
        values: {
          scenario: projectLiveSectionSettings.scenario.value,
          host: projectLiveSectionSettings.host.value,
          mode: next,
        },
      });
    }),
  );
  return () => {
    demoListeners.dispose();
  };
}

/** The catalog entries this file renders. */
export const demoModule: DemoModule = {
  entries: {
    'project-live-section': {
      catalogExamples: true,
      Demo: ProjectLiveSectionDemo,
      Settings: ProjectLiveSectionSettings,
      reset: resetProjectLiveSectionDemo,
    },
  },
  wire: wireProjectLiveSectionDemo,
};
