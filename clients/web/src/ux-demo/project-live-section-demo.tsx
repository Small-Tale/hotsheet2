import { Button } from '@kerfjs/ui/button';
import { CatalogExample, CatalogExampleStack } from '@kerfjs/ui/catalog';
import { Select, type SelectChoice } from '@kerfjs/ui/select';
import { delegate, signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import { LiveCiContextMenu, type LiveCiMenuState } from '../components/live-ci-context-menu';
import { wireLiveCiMenu } from '../components/live-ci-menu-interactions';
import { ProjectLiveSection } from '../components/project-live-section';
import { COMMANDS_AND_AI_ACTIONS } from '../interaction-attrs/commands-and-ai';
import { hostCallouts, projectLiveSummary } from '../project-live-summary';
import type { DemoModule, FormControl } from './demo-module';
import { DemoSettingsForm } from './demo-settings-form';
import {
  DEMO_LIVE_NOW_MS,
  type DemoCiScenario,
  demoHostHealth,
  type DemoHostScenario,
  demoLiveCiRuns,
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

// The CI runs (HS2-505PHC): each compact chip names itself and, while it runs, its elapsed time; HS2-D3JJ49: one
// chip per run in flight, and nothing for a run that cannot be verified.
const CI_CHOICES: SelectChoice[] = [
  { value: 'none', label: 'None Reported' },
  { value: 'running', label: 'Running' },
  { value: 'running-no-start', label: 'Running, No Start Reported' },
  { value: 'queued', label: 'Queued' },
  { value: 'failed', label: 'Failed' },
  { value: 'passed', label: 'Passed' },
  { value: 'transient', label: 'Running, Provider Unreachable' },
  { value: 'several', label: 'Several Runs in Flight' },
  { value: 'unverifiable', label: 'Cannot Be Verified' },
];

const MODE_CHOICES: SelectChoice[] = [
  { value: 'compact', label: 'Compact (Default)' },
  { value: 'expanded', label: 'Expanded Details' },
];

const DEFAULTS = { scenario: 'attention', host: 'disk-warning', ci: 'running', mode: 'compact' } as const;

/** Every CI chip face, side by side in the states example. */
const CI_CHIP_STATES: readonly DemoCiScenario[] = [
  'running',
  'running-no-start',
  'queued',
  'failed',
  'transient',
  'several',
];

export const projectLiveSectionSettings = {
  scenario: signal<string>(DEFAULTS.scenario),
  host: signal<string>(DEFAULTS.host),
  ci: signal<string>(DEFAULTS.ci),
  mode: signal<string>(DEFAULTS.mode),
};

/** HS2-7PSBHQ: the open Live CI context menu of the interactive example. */
export const projectLiveSectionCiMenu = signal<LiveCiMenuState | undefined>(undefined);

/** HS2-D3JJ49: the runs Clear CI Status removed from the interactive example, by branch and workflow. */
export const projectLiveSectionClearedRuns = signal<readonly string[]>([]);

const runKey = (run: { branch: string | null; workflow: string | null }) => `${run.branch ?? ''}:${run.workflow ?? ''}`;

export function resetProjectLiveSectionDemo(root?: ParentNode): void {
  projectLiveSectionSettings.scenario.value = DEFAULTS.scenario;
  projectLiveSectionSettings.host.value = DEFAULTS.host;
  projectLiveSectionSettings.ci.value = DEFAULTS.ci;
  projectLiveSectionSettings.mode.value = DEFAULTS.mode;
  projectLiveSectionCiMenu.value = undefined;
  projectLiveSectionClearedRuns.value = [];
  if (root) syncSettingsControls(root, 'project-live-section', { values: { ...DEFAULTS } });
}

function liveState(scenario: string, ci: string, cleared: readonly string[] = []) {
  const state = demoLiveState(scenario as DemoLiveScenario),
    runs = demoLiveCiRuns(ci as DemoCiScenario).filter((run) => !cleared.includes(runKey(run)));
  return ci === 'none' ? state : { ...state, ...(runs[0] ? { ci: runs[0] } : {}), ci_runs: runs };
}

function section(scenario: string, host: string, ci: string, expanded: boolean, cleared: readonly string[] = []) {
  return (
    <ProjectLiveSection
      summary={
        scenario === 'none' ? undefined : projectLiveSummary([liveState(scenario, ci, cleared)], DEMO_LIVE_NOW_MS)
      }
      callouts={hostCallouts(demoHostHealth(host as DemoHostScenario))}
      expanded={expanded}
    />
  );
}

/** The interactive section; Settings switch the live state, host health, `main` CI, and mode. */
export function ProjectLiveSectionDemo() {
  const { scenario, host, ci, mode } = projectLiveSectionSettings;
  return (
    <CatalogExampleStack>
      <CatalogExample
        label="Interactive"
        note="The header action switches between the compact line and the detail rows. Hover a gauge or chip for its explanation, which is also its accessible name. Right-click a CI chip or row for Clear CI Status, which forgets that run; right-click elsewhere on the section, or press the context-menu key on it, to list each shown run (HS2-7PSBHQ, HS2-D3JJ49)."
      >
        <div class="demo-sidebar-width">
          {section(
            scenario.value,
            host.value,
            ci.value,
            mode.value === 'expanded',
            projectLiveSectionClearedRuns.value,
          )}
        </div>
        {projectLiveSectionCiMenu.value && <LiveCiContextMenu {...projectLiveSectionCiMenu.value} />}
      </CatalogExample>

      <CatalogExample
        label="Main CI Chip"
        note="The compact CI chip names itself (HS2-505PHC): its elapsed time while it runs, which the app re-renders each second while the section shows, then Running before a start is reported, Queued, and Failed. A run whose provider is unreachable for now keeps its last face. Several runs in flight each get a chip named by their workflow (HS2-D3JJ49). A passing run, or one that cannot be verified, shows no chip."
      >
        {CI_CHIP_STATES.map((state) => (
          <div class="demo-sidebar-width" data-demo-ci={state}>
            {section('quiet', 'normal', state, false)}
          </div>
        ))}
      </CatalogExample>
    </CatalogExampleStack>
  );
}

export function ProjectLiveSectionSettings() {
  const { scenario, host, ci, mode } = projectLiveSectionSettings;
  return (
    <DemoSettingsForm settings="project-live-section">
      <Select name="scenario" label="Live State" value={scenario.value} choices={SCENARIO_CHOICES} />
      <Select name="host" label="Host Health" value={host.value} choices={HOST_CHOICES} />
      <Select name="ci" label="Main CI" value={ci.value} choices={CI_CHOICES} />
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
      if (name === 'ci') {
        projectLiveSectionSettings.ci.value = control.value;
        projectLiveSectionClearedRuns.value = [];
      }
      if (name === 'mode') projectLiveSectionSettings.mode.value = control.value;
    }),
  );
  demoListeners.add(
    delegate(root, 'click', COMMANDS_AND_AI_ACTIONS.toggleLiveSection.selector, (_event, target) => {
      // Only the interactive example switches modes; the CI chip states stay compact.
      if (!target.closest('.demo-sidebar-width') || target.closest('[data-demo-ci]')) return;
      const next = projectLiveSectionSettings.mode.value === 'expanded' ? 'compact' : 'expanded';
      projectLiveSectionSettings.mode.value = next;
      syncSettingsControls(root, 'project-live-section', {
        values: {
          scenario: projectLiveSectionSettings.scenario.value,
          host: projectLiveSectionSettings.host.value,
          ci: projectLiveSectionSettings.ci.value,
          mode: next,
        },
      });
    }),
  );
  // HS2-7PSBHQ, HS2-D3JJ49: the same menu wiring as the app; clearing here removes that run of the
  // interactive example.
  demoListeners.add(
    wireLiveCiMenu(root, {
      menu: projectLiveSectionCiMenu,
      clear: (target) => {
        projectLiveSectionClearedRuns.value = [
          ...projectLiveSectionClearedRuns.value,
          runKey({ branch: target.branch ?? null, workflow: target.workflow ?? null }),
        ];
      },
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
