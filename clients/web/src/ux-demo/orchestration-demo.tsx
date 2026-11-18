import { Button } from '@kerfjs/ui/button';
import { CatalogExample } from '@kerfjs/ui/catalog';
import { Checkbox } from '@kerfjs/ui/checkbox';
import { Select } from '@kerfjs/ui/select';
import { delegate, signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import { OrchestrationDecisionsDialog } from '../components/orchestration-decisions-dialog';
import { OrchestrationInspector } from '../components/orchestration-inspector';
import { wireOrchestrationTimeline } from '../components/orchestration-timeline-interactions';
import { OrchestrationTimelineView } from '../components/orchestration-timeline-view';
import { OrchestrationToolbar } from '../components/orchestration-toolbar';
import { TicketRow } from '../components/ticket-row';
import type { TimelinePhase } from '../orchestration-timeline';
import { findTimelineItem, timelineCard } from '../orchestration-timeline-board';
import type { DemoModule, FormControl } from './demo-module';
import { DemoSettingsForm } from './demo-settings-form';
import { demoClock, demoContention, demoDecisions, demoSummary, demoTimeline } from './orchestration-fixtures';
import { syncSettingsControls } from './settings-controls';

/** HS2-J89JC7: the Orchestration view's production components with synthetic timelines. */
export const orchestrationDemoSettings = {
  phase: signal<TimelinePhase>('running'),
  selected: signal('step-3'),
  proposeAvailable: signal(true),
  /** HS2-5RYXNN, HS2-FDS95D: the planning session's state: none, working, finished its turn, or waiting. */
  planning: signal(''),
  busy: signal(false),
  collapsed: signal<string[]>([]),
};

const PHASES: ReadonlyArray<{ value: TimelinePhase; label: string }> = [
  { value: 'running', label: 'Running' },
  { value: 'paused', label: 'Paused' },
  { value: 'proposed', label: 'Proposed Plan' },
  { value: 'no_plan', label: 'No Plan Yet' },
  { value: 'no_projects', label: 'No Projects Yet' },
];

const PLANNING = [
  { value: '', label: 'None' },
  { value: 'planning', label: 'Working' },
  { value: 'replannable', label: 'Finished Its Turn' },
  { value: 'waiting', label: 'Waiting in Its Terminal' },
];

const SELECTIONS = [
  { value: 'step-3', label: 'API-12 Running' },
  { value: 'step-5', label: 'DOC-7 Provisional' },
  { value: 'step-6', label: 'API-15 Waits for Permission' },
  { value: 'step-7', label: 'WEB-30 Waits for a Decision' },
  { value: 'step-8', label: 'DOC-9 Ready' },
  { value: 'step-9', label: 'WEB-35 Waits on a Prerequisite' },
  { value: 'step-1', label: 'API-11 Done' },
];

export function resetOrchestrationDemo(root?: ParentNode): void {
  const settings = orchestrationDemoSettings;
  settings.phase.value = 'running';
  settings.selected.value = 'step-3';
  settings.proposeAvailable.value = true;
  settings.planning.value = '';
  settings.busy.value = false;
  settings.collapsed.value = [];
  if (root)
    syncSettingsControls(root, 'orchestration', {
      values: { phase: settings.phase.value, selected: settings.selected.value, planning: settings.planning.value },
      checked: {
        'propose-available': settings.proposeAvailable.value,
        busy: settings.busy.value,
      },
    });
}

const propose = () =>
  orchestrationDemoSettings.proposeAvailable.value
    ? { available: true }
    : { available: false, reason: 'Choose an AI tool for this project in Settings first' };

const card = (step: string) => {
  const timeline = demoTimeline('running');
  return timelineCard(findTimelineItem(timeline, step)!.item, timeline, undefined, demoClock);
};

/** The work area: wide (grid, opened at its right end in the app) and phone (paged). */
export function OrchestrationTimelineViewDemo() {
  const timeline = demoTimeline(orchestrationDemoSettings.phase.value),
    view = (layout: 'grid' | 'paged') => (
      <OrchestrationTimelineView
        timeline={timeline}
        layout={layout}
        selectedStep={orchestrationDemoSettings.selected.value}
        collapsedGroups={orchestrationDemoSettings.collapsed.value}
        propose={propose()}
        clock={demoClock}
      />
    );
  return (
    <>
      <CatalogExample label="Wide" viewport={{ width: 'full', height: 'app', frame: 'solid', overflow: 'hidden' }}>
        {view('grid')}
      </CatalogExample>
      <CatalogExample label="Phone" viewport={{ width: 'compact', height: 'app', frame: 'solid', overflow: 'hidden' }}>
        {view('paged')}
      </CatalogExample>
      {/* The shared TicketRow's orchestration line, as a list row and as a narrow card. */}
      <CatalogExample label="Card Orchestration Line" viewport={{ width: 'full' }}>
        <TicketRow {...card('step-6')} presentation="list" listEdge="only" />
      </CatalogExample>
      <CatalogExample label="Card Orchestration Line, Narrow" viewport={{ width: 'compact' }}>
        <TicketRow {...card('step-7')} presentation="column" />
      </CatalogExample>
      {/* HS2-F1BKQ0: this machine kept a step differently from the checked-in plan. */}
      <CatalogExample
        label="Plan Conflict"
        viewport={{ width: 'full', height: 'app', frame: 'solid', overflow: 'hidden' }}
      >
        <OrchestrationTimelineView
          error="plan_conflict"
          planHealth={{
            state: 'conflict',
            conflicts: ['step-3'],
            local: ['step-3'],
            discard_resolves: true,
          }}
          stepName={() => 'API-12'}
        />
      </CatalogExample>
      <CatalogExample label="Loading" viewport={{ width: 'full', height: 'app', frame: 'solid', overflow: 'hidden' }}>
        <OrchestrationTimelineView loading />
      </CatalogExample>
      <CatalogExample
        label="Read Failed"
        viewport={{ width: 'full', height: 'app', frame: 'solid', overflow: 'hidden' }}
      >
        <OrchestrationTimelineView
          timeline={timeline}
          error="The server did not answer."
          propose={propose()}
          clock={demoClock}
        />
      </CatalogExample>
    </>
  );
}

/** The page header: status and the one kick-off action of each phase. */
export function OrchestrationToolbarDemo() {
  return (
    <>
      {PHASES.map(({ value, label }) => (
        <CatalogExample label={label}>
          <OrchestrationToolbar
            timeline={demoTimeline(value)}
            propose={propose()}
            busy={orchestrationDemoSettings.busy.value}
            planning={orchestrationDemoSettings.planning.value || undefined}
            clock={demoClock}
          />
        </CatalogExample>
      ))}
    </>
  );
}

/** The inspector of the selected card, in the state the settings choose. */
export function OrchestrationInspectorDemo() {
  const timeline = demoTimeline('running'),
    found = findTimelineItem(timeline, orchestrationDemoSettings.selected.value);
  return (
    <>
      <CatalogExample label="Selected Card" viewport={{ width: 'compact', height: 'app', frame: 'solid' }}>
        <OrchestrationInspector
          item={found?.item}
          timeline={timeline}
          busy={orchestrationDemoSettings.busy.value}
          clock={demoClock}
          details={{
            loading: false,
            ci: 'pending',
            gates: [
              { kind: 'ci', state: 'pending', provider: 'github-actions', commit: '3f9c2a1d00' },
              // HS2-GD3NNK: a requested code review, in the review's own words.
              { kind: 'review', state: 'requested', provider: 'review', commit: '3f9c2a1d00' },
            ],
            validations: [
              {
                step_id: 'contract-check',
                check: 'profile',
                state: 'passed',
                at_ms: timeline.now_ms - 120_000,
                exit_code: 0,
                evidence: '4f2a91c',
              },
              {
                step_id: 'smoke',
                check: 'command',
                state: 'failed',
                reason: 'exit_code_mismatch',
                at_ms: timeline.now_ms - 60_000,
                exit_code: 1,
                evidence: '9c03be1',
              },
              { step_id: 'e2e-login', check: 'profile', state: 'skipped', reason: 'prerequisite_not_passed' },
            ],
            runRead: true,
            // HS2-77SRHC, HS2-5GRXFN: DOC-9 is ready and waits for the VM someone holds.
            explanation:
              orchestrationDemoSettings.selected.value === 'step-8'
                ? {
                    standing: 'not_placed',
                    reason: 'resource_held',
                    retry_at_ms: timeline.now_ms + 40 * 60_000,
                    resources: [
                      {
                        namespace: 'vm',
                        resource: 'macos-vm',
                        state: 'held',
                        expires_at_ms: timeline.now_ms + 40 * 60_000,
                      },
                    ],
                  }
                : undefined,
            // HS2-0D60T8: DOC-7 was built ahead of WEB-33's CI, which failed.
            builtAhead:
              orchestrationDemoSettings.selected.value === 'step-5'
                ? {
                    state: 'at_risk',
                    depth: 1,
                    max_depth: 2,
                    recovery: 'rebase_onto_fix',
                    held: true,
                    gates: [
                      {
                        target_id: 'web',
                        connection_id: 'web-tickets',
                        native_id: 'ticket-WEB-33',
                        commit: '3f9c2a1d00',
                        result: 'red',
                      },
                    ],
                  }
                : undefined,
            reconciliation: {
              at_ms: timeline.now_ms - 120_000,
              disposition: 'continue',
              findings: [{ source: 'ci_run', verification: 'unverifiable', reason: 'provider_unreachable' }],
            },
            events: [
              { at_ms: timeline.now_ms - 60_000, kind: 'gate', state: 'pending', source: 'target' },
              { at_ms: timeline.now_ms - 300_000, kind: 'session', reason: 'started', source: 'orchestrator' },
              { at_ms: timeline.now_ms - 300_000, kind: 'state', state: 'dispatched', source: 'orchestrator' },
            ],
          }}
        />
      </CatalogExample>
      <CatalogExample label="Nothing Selected: Overview" viewport={{ width: 'compact', height: 'app', frame: 'solid' }}>
        <OrchestrationInspector
          timeline={timeline}
          projectName="Launch"
          clock={demoClock}
          overview={{ decisions: demoDecisions(), contention: demoContention(), summary: demoSummary() }}
        />
      </CatalogExample>
      <CatalogExample label="No Timeline Yet" viewport={{ width: 'compact', height: 'app', frame: 'solid' }}>
        <OrchestrationInspector />
      </CatalogExample>
      {/* HS2-77SRHC: Show All Decisions… opens this dialog over the view; the catalog shows it in place. */}
      <CatalogExample label="Show All Decisions…" viewport={{ width: 'full' }}>
        <OrchestrationDecisionsDialog
          presentation="embedded"
          state={{ entries: demoDecisions().entries, more: true, loading: false }}
          timeline={timeline}
          clock={demoClock}
        />
      </CatalogExample>
    </>
  );
}

export function OrchestrationDemoSettings() {
  const settings = orchestrationDemoSettings;
  return (
    <DemoSettingsForm settings="orchestration">
      <Select name="phase" label="Phase" value={settings.phase.value} choices={PHASES} />
      <Select name="selected" label="Selected Card" value={settings.selected.value} choices={SELECTIONS} />
      <Checkbox name="propose-available" checked={settings.proposeAvailable.value} label="Planner Available" />
      <Select name="planning" label="Planning Session" value={settings.planning.value} choices={PLANNING} />
      <Checkbox name="busy" checked={settings.busy.value} label="Change in Progress" />
      <Button type="button" action={'reset-settings'}>
        Reset
      </Button>
    </DemoSettingsForm>
  );
}

/** The settings form, and the production view wiring with fixture bindings. */
function wireOrchestrationDemo(root: HTMLElement): () => void {
  const settings = orchestrationDemoSettings,
    scope = createScope();
  scope.add(
    delegate(root, 'change', '[data-settings="orchestration"] [name]', (_event, target) => {
      const control = target as FormControl,
        name = control.getAttribute('name');
      if (name === 'phase') settings.phase.value = control.value as TimelinePhase;
      if (name === 'selected') settings.selected.value = control.value;
      if (name === 'propose-available') settings.proposeAvailable.value = control.checked;
      if (name === 'planning') settings.planning.value = control.value;
      if (name === 'busy') settings.busy.value = control.checked;
    }),
  );
  scope.add(
    wireOrchestrationTimeline(root, {
      select: (step) => {
        settings.selected.value = step;
      },
      toggleGroup: (group) => {
        const current = settings.collapsed.value;
        settings.collapsed.value = current.includes(group)
          ? current.filter((item) => item !== group)
          : [...current, group];
      },
      start: () => {
        settings.phase.value = 'running';
      },
      pause: () => {
        settings.phase.value = 'paused';
      },
      propose: () => {
        settings.planning.value = 'planning';
      },
      chooseProjects: () => undefined,
      runControl: () => undefined,
      showRequest: () => undefined,
      openGoal: () => undefined,
      runValidation: () => undefined,
      reopen: () => undefined,
      showDecisions: () => undefined,
      loadEarlierDecisions: () => undefined,
      closeDecisions: () => undefined,
      keepCheckedInPlan: () => undefined,
    }),
  );
  return () => {
    scope.dispose();
  };
}

/** The catalog entries this file renders (HS2-J89JC7). */
export const demoModule: DemoModule = {
  entries: {
    'orchestration-timeline-view': {
      catalogExamples: true,
      stackWidth: 'full',
      Demo: OrchestrationTimelineViewDemo,
      Settings: OrchestrationDemoSettings,
      reset: resetOrchestrationDemo,
    },
    'orchestration-toolbar': {
      catalogExamples: true,
      stackWidth: 'full',
      Demo: OrchestrationToolbarDemo,
      Settings: OrchestrationDemoSettings,
      reset: resetOrchestrationDemo,
    },
    'orchestration-inspector': {
      catalogExamples: true,
      Demo: OrchestrationInspectorDemo,
      Settings: OrchestrationDemoSettings,
      reset: resetOrchestrationDemo,
    },
  },
  wire: wireOrchestrationDemo,
};
