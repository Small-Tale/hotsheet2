import { Button } from '@kerfjs/ui/button';
import { CatalogExample } from '@kerfjs/ui/catalog';
import { Checkbox } from '@kerfjs/ui/checkbox';
import { Select } from '@kerfjs/ui/select';
import { delegate, signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import { OrchestrationDecisionsDialog } from '../components/orchestration-decisions-dialog';
import { OrchestrationInspector } from '../components/orchestration-inspector';
import {
  OrchestrationTemplateDialog,
  type OrchestrationTemplateDialogState,
} from '../components/orchestration-template-dialog';
import { wireOrchestrationTimeline } from '../components/orchestration-timeline-interactions';
import { OrchestrationTimelineView } from '../components/orchestration-timeline-view';
import { OrchestrationToolbar } from '../components/orchestration-toolbar';
import {
  OrchestrationTriggerDialog,
  type OrchestrationTriggerDialogState,
} from '../components/orchestration-trigger-dialog';
import { TicketRow } from '../components/ticket-row';
import { CI_WATCH_REASONS } from '../orchestration-run-checks';
import type { OrchestrationTimeline, TimelineItem, TimelinePhase } from '../orchestration-timeline';
import { findTimelineItem, timelineCard } from '../orchestration-timeline-board';
import { newTriggerForm } from '../orchestration-triggers';
import type { DemoModule, FormControl } from './demo-module';
import { DemoSettingsForm } from './demo-settings-form';
import {
  DEMO_NOW,
  demoClock,
  demoContention,
  demoDecisions,
  demoHomeOutages,
  demoIntegrator,
  demoOverlapTrigger,
  demoRoles,
  demoSummary,
  demoTemplateApply,
  demoTemplateHeld,
  demoTemplatePreview,
  demoTemplates,
  demoTimeline,
  demoTriggerReview,
  demoTriggers,
} from './orchestration-fixtures';
import { syncSettingsControls } from './settings-controls';

/** HS2-J89JC7: the Orchestration view's production components with synthetic timelines. */
export const orchestrationDemoSettings = {
  phase: signal<TimelinePhase>('running'),
  selected: signal('step-3'),
  proposeAvailable: signal(true),
  /** HS2-5RYXNN, HS2-FDS95D: the planning session's state: none, working, finished its turn, or waiting. */
  planning: signal(''),
  busy: signal(false),
  /** HS2-YSRT6Y: no explicit goal is open, so planning works through all open tickets. */
  defaultGoal: signal(false),
  /** HS2-R2N33M: placement runs because the policy leaves it Auto and a plan exists. */
  autoPlacement: signal(false),
  /** HS2-D9KP86: no policy is saved, so Start Orchestration saves the defaults and the banner leads to them. */
  policyAbsent: signal(false),
  /** HS2-38R8J2: placement was never on, so a paused plan reads Not Started under its dry-run banner. */
  neverStarted: signal(false),
  /** HS2-FXY5QM: the condition of API-12's run: '' is a worker at work. */
  runCondition: signal(''),
  /** HS2-65AD6J: how the project's integrator stands for a card with a run; '' shows no Integration section. */
  integrator: signal('keep'),
  /** HS2-747WQA: what the ready DOC-9 waits for: a held resource, or a Models pattern that resolves to no model. */
  docWait: signal('resource_held'),
  /** HS2-A7YCB2: API-12's CI gate: '' is pending; an unknown reason offers Link Again. */
  ciGate: signal(''),
  /**
   * HS2-89578K: the done API-11's CI verdict: '' is none linked; `failed` reads as needing repair. HS2-GPABYY:
   * `deploy_failed` and `changes_requested` fail its deployment or its review instead.
   */
  doneCi: signal(''),
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
  // HS2-190RDX: a plan update drafts while the current plan runs.
  { value: 'updating', label: 'Updating the Plan' },
  // HS2-N5BDE4: the planner asked the person a question on a goal ticket.
  { value: 'asked', label: 'Asked a Question' },
];

const INTEGRATOR = [
  { value: '', label: 'Not Read' },
  { value: 'start', label: 'Starts' },
  { value: 'keep', label: 'Running' },
  { value: 'wind_down', label: 'Winding Down' },
  { value: 'not_started', label: 'Not Started: Session Limit' },
  { value: 'opt_out', label: 'Not Started: Integrates by Hand' },
  // HS2-747WQA: the project's Models pattern resolves to no model its machine reports.
  { value: 'no_model', label: 'Not Started: No Model Resolves' },
  // HS2-GK6HC8: every AI tool it may run is signed out on the home's machine.
  { value: 'signed_out', label: 'Not Started: AI Tool Signed Out' },
  { value: 'unavailable', label: 'Unavailable' },
  { value: 'none', label: 'Not Needed' },
];

const RUN_CONDITIONS = [
  { value: '', label: 'A Worker at Work' },
  { value: 'intervened', label: 'No Worker Holds It' },
  { value: 'paused', label: 'Paused' },
  { value: 'cancelling', label: 'Cancelling' },
  { value: 'unreachable', label: 'Project Unreachable' },
  // HS2-9FS7YX: the scheduler holds the step back: its sessions ended before they took up the work, or the last
  // one is kept for a person.
  { value: 'held', label: 'Held: Sessions Never Joined' },
  { value: 'unjoined', label: 'Session Kept Without Joining' },
  // HS2-JGNCAR: the project's home gave the ticket to its own worker; the session placed here is free.
  { value: 'elsewhere', label: 'Another Worker Took It' },
];

/** HS2-9FS7YX: API-12 as the scheduler holds it back, its latest decision logged after its run started. */
function heldItem(item: TimelineItem, condition: string): TimelineItem {
  const at = DEMO_NOW - 60_000;
  if (condition === 'held')
    return {
      ...item,
      session_state: 'lost',
      placement: {
        at_ms: at,
        decision: 'unplaced',
        reason: 'never_joined',
        detail: 'replacements_exhausted',
        retry_at_ms: DEMO_NOW + 58 * 60_000,
      },
    };
  return {
    ...item,
    placement: { at_ms: at, decision: 'keep', reason: 'never_joined', detail: 'replacements_exhausted' },
  };
}

/** HS2-89578K: the done API-11's run with the chosen CI verdict. */
function withDoneCi(timeline: OrchestrationTimeline): OrchestrationTimeline {
  const ci = orchestrationDemoSettings.doneCi.value;
  if (!ci) return timeline;
  return {
    ...timeline,
    waves: timeline.waves.map((wave) => ({
      ...wave,
      items: wave.items.map((item) =>
        item.step_id === 'step-1' && item.run ? { ...item, run: { ...item.run, ...DONE_GATES[ci] } } : item,
      ),
    })),
  };
}

/** HS2-36XDDX: DOC-9 held for its template's accept, when the settings choose that wait. */
function withHeldTemplate(timeline: OrchestrationTimeline): OrchestrationTimeline {
  if (orchestrationDemoSettings.docWait.value !== 'template_plan_not_accepted') return timeline;
  return {
    ...timeline,
    waves: timeline.waves.map((wave) => ({
      ...wave,
      items: wave.items.map((item) => (item.step_id === 'step-8' ? demoTemplateHeld(item) : item)),
    })),
  };
}

/** HS2-FXY5QM: the running timeline with API-12's run in the chosen condition. */
function runningTimeline(phase: TimelinePhase = 'running') {
  const timeline = withSignedOut(withHeldTemplate(withDoneCi(demoTimeline(phase)))),
    condition = orchestrationDemoSettings.runCondition.value;
  if (!condition) return timeline;
  if (condition === 'elsewhere')
    return {
      ...timeline,
      waves: timeline.waves.map((wave) => ({
        ...wave,
        items: wave.items.map((item) =>
          item.step_id === 'step-3' && item.run
            ? {
                ...item,
                tool: undefined,
                model: undefined,
                session_id: undefined,
                session_state: undefined,
                worked_elsewhere: true,
                run: { ...item.run, worker_id: 'home-worker-1' },
              }
            : item,
        ),
      })),
    };
  if (condition === 'held' || condition === 'unjoined')
    return {
      ...timeline,
      waves: timeline.waves.map((wave) => ({
        ...wave,
        items: wave.items.map((item) => (item.step_id === 'step-3' ? heldItem(item, condition) : item)),
      })),
    };
  const change = (run: NonNullable<TimelineItem['run']>) => ({
    ...run,
    state: condition === 'intervened' ? 'intervened' : run.state,
    paused: condition === 'paused' || run.paused,
    cancel_requested: condition === 'cancelling' || run.cancel_requested,
    unreachable_since_ms: condition === 'unreachable' ? DEMO_NOW - 600_000 : run.unreachable_since_ms,
  });
  return {
    ...timeline,
    waves: timeline.waves.map((wave) => ({
      ...wave,
      items: wave.items.map((item) =>
        item.step_id === 'step-3' && item.run ? { ...item, run: change(item.run) } : item,
      ),
    })),
  };
}

const DOC_WAITS = [
  { value: 'resource_held', label: 'A Held Resource' },
  { value: 'model_unresolved', label: 'Models Not Reported' },
  { value: 'no_matching_model', label: 'No Matching Model' },
  // HS2-36XDDX: its template holds it until someone accepts the proposed plan.
  { value: 'template_plan_not_accepted', label: 'Its Template’s Accept' },
  // HS2-ZRPZJJ: its AI tool is signed out on its machine (and on API's).
  { value: 'tool_signed_out', label: 'Its AI Tool Signed Out' },
];

/** HS2-ZRPZJJ: the scheduler's detail when DOC-9's AI tool is signed out on its machine. */
const DEMO_SIGNED_OUT = 'tool_signed_out:claude';

/** HS2-ZRPZJJ: DOC-9's last pass found Claude signed out, when the settings choose that wait. */
function withSignedOut(timeline: OrchestrationTimeline): OrchestrationTimeline {
  if (orchestrationDemoSettings.docWait.value !== 'tool_signed_out') return timeline;
  const placement = {
    at_ms: DEMO_NOW - 60_000,
    decision: 'unplaced' as const,
    reason: 'no_tool_available',
    detail: DEMO_SIGNED_OUT,
  };
  return {
    ...timeline,
    waves: timeline.waves.map((wave) => ({
      ...wave,
      items: wave.items.map((item) => (item.step_id === 'step-8' ? { ...item, placement } : item)),
    })),
  };
}

const CI_GATES = [
  { value: '', label: 'Pending' },
  { value: 'not_watched', label: 'Not Watched' },
  { value: 'watch_ended', label: 'Watch Ended' },
  { value: 'watch_gone', label: 'Watch Gone' },
  // HS2-2751D6: the gate failed, and the Gates section says what to do.
  { value: 'failed', label: 'Failed' },
];

/** HS2-A7YCB2: API-12's CI gate in the chosen state, as the view parses it. */
function demoCiGate() {
  const reason = orchestrationDemoSettings.ciGate.value,
    commit = '3f9c2a1d00';
  if (!reason) return { kind: 'ci', state: 'pending', provider: 'github-actions', commit, gate_id: 'gate-ci' };
  if (reason === 'failed')
    return { kind: 'ci', state: 'failed', provider: 'github-actions', commit, gate_id: 'gate-ci' };
  return {
    kind: 'ci',
    state: `unknown: ${CI_WATCH_REASONS[reason]}`,
    provider: 'github-actions',
    commit,
    gate_id: 'gate-ci',
    relink: { commit, provider: 'github-actions', repository: 'acme/synthetic-api', workflow: 'CI', branch: 'main' },
  };
}

const DONE_CI = [
  { value: '', label: 'None Linked' },
  { value: 'passed', label: 'Passed' },
  { value: 'failed', label: 'Failed' },
  { value: 'deploy_failed', label: 'Passed, Deploy Failed' },
  { value: 'changes_requested', label: 'Passed, Changes Requested' },
];

/** HS2-GPABYY: the run's gate verdicts each Done API-11 choice sets. */
const DONE_GATES: Readonly<Record<string, { ci_gate: string; deployment_gate?: string; review_gate?: string }>> = {
  passed: { ci_gate: 'passed' },
  failed: { ci_gate: 'failed' },
  deploy_failed: { ci_gate: 'passed', deployment_gate: 'failed' },
  changes_requested: { ci_gate: 'passed', review_gate: 'failed' },
};

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
  settings.defaultGoal.value = false;
  settings.autoPlacement.value = false;
  settings.policyAbsent.value = false;
  settings.neverStarted.value = false;
  settings.runCondition.value = '';
  settings.integrator.value = 'keep';
  settings.docWait.value = 'resource_held';
  settings.ciGate.value = '';
  settings.doneCi.value = '';
  settings.collapsed.value = [];
  if (root)
    syncSettingsControls(root, 'orchestration', {
      values: {
        phase: settings.phase.value,
        selected: settings.selected.value,
        planning: settings.planning.value,
        integrator: settings.integrator.value,
        'doc-wait': settings.docWait.value,
        'ci-gate': settings.ciGate.value,
        'done-ci': settings.doneCi.value,
        'run-condition': settings.runCondition.value,
      },
      checked: {
        'propose-available': settings.proposeAvailable.value,
        busy: settings.busy.value,
        'default-goal': settings.defaultGoal.value,
        'auto-placement': settings.autoPlacement.value,
        'policy-absent': settings.policyAbsent.value,
        'never-started': settings.neverStarted.value,
      },
    });
}

const propose = () =>
  orchestrationDemoSettings.proposeAvailable.value
    ? { available: true }
    : { available: false, reason: 'Choose an AI tool for this project in Settings first' };

const card = (step: string) => {
  const timeline = runningTimeline();
  return timelineCard(findTimelineItem(timeline, step)!.item, timeline, undefined, demoClock);
};

/** The work area: wide (grid, opened at its right end in the app) and phone (paged). */
export function OrchestrationTimelineViewDemo() {
  const timeline = runningTimeline(orchestrationDemoSettings.phase.value),
    view = (layout: 'grid' | 'paged') => (
      <OrchestrationTimelineView
        timeline={timeline}
        layout={layout}
        selectedStep={orchestrationDemoSettings.selected.value}
        collapsedGroups={orchestrationDemoSettings.collapsed.value}
        propose={propose()}
        planning={orchestrationDemoSettings.planning.value}
        showPlanner={orchestrationDemoSettings.planning.value === 'waiting'}
        busy={orchestrationDemoSettings.busy.value}
        defaultGoal={orchestrationDemoSettings.defaultGoal.value}
        templates
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
      {/* HS2-EX594V: a multi-site project's home site is unavailable; there is no failover. */}
      <CatalogExample
        label="Home Site Unavailable"
        viewport={{ width: 'full', height: 'app', frame: 'solid', overflow: 'hidden' }}
      >
        <OrchestrationTimelineView
          timeline={{ ...timeline, home_outages: demoHomeOutages() }}
          propose={propose()}
          clock={demoClock}
        />
      </CatalogExample>
      <CatalogExample
        label="Home Site Unavailable, Phone"
        viewport={{ width: 'compact', height: 'app', frame: 'solid', overflow: 'hidden' }}
      >
        <OrchestrationTimelineView
          timeline={{ ...timeline, home_outages: demoHomeOutages().slice(0, 1) }}
          layout="paged"
          propose={propose()}
          clock={demoClock}
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
            timeline={{
              // HS2-0YHA2S: with DOC-9 held for its template's accept, the running header counts it and
              // offers Accept Plan (a held step exists only while placement is on).
              ...(value === 'running' ? withHeldTemplate(demoTimeline(value)) : demoTimeline(value)),
              automatic_by_default: value === 'running' && orchestrationDemoSettings.autoPlacement.value,
              never_started: value === 'paused' && orchestrationDemoSettings.neverStarted.value,
              ...((value === 'proposed' || value === 'paused') && orchestrationDemoSettings.policyAbsent.value
                ? { kick_off: { ...demoTimeline(value).kick_off, default_policy: true } }
                : {}),
            }}
            propose={propose()}
            update={propose()}
            question={orchestrationDemoSettings.planning.value === 'asked' ? 'LCH-1' : undefined}
            defaultGoal={orchestrationDemoSettings.defaultGoal.value}
            busy={orchestrationDemoSettings.busy.value}
            planning={orchestrationDemoSettings.planning.value || undefined}
            showPlanner={orchestrationDemoSettings.planning.value === 'waiting'}
            templates
            triggers
            clock={demoClock}
          />
        </CatalogExample>
      ))}
      <OrchestrationTemplateDialogDemo />
      <OrchestrationTriggerDialogDemo />
    </>
  );
}

/** HS2-A4RVYA: the Use Template… dialog the header opens, in place, in each of its states. */
function templateState(change: Partial<OrchestrationTemplateDialogState> = {}): OrchestrationTemplateDialogState {
  const templates = demoTemplates();
  return {
    loading: false,
    templates,
    selected: templates[0].template_id,
    mode: 'template',
    detailActive: false,
    inputs: { feature: 'Rate limits', priority: 'high', api: 'api', web: 'web', contract: true },
    changed: [],
    choices: {},
    preview: demoTemplatePreview(),
    previewing: false,
    busy: false,
    projects: [
      { target_id: 'api', label: 'API · Build host' },
      { target_id: 'web', label: 'Web · This computer' },
      { target_id: 'docs', label: 'Docs · This computer' },
    ],
    save: { goal: 'ticket-LCH-1', name: '', description: '', targetsAsInputs: true },
    goals: [
      {
        connection_id: 'launch-tickets',
        native_id: 'ticket-LCH-1',
        slug: 'LCH-1',
        title: 'Launch the synthetic sign-on',
        steps: 3,
      },
    ],
    session: 1,
    ...change,
  };
}

function OrchestrationTemplateDialogDemo() {
  const examples: Array<[string, OrchestrationTemplateDialogState, boolean]> = [
    ['Use Template…', templateState(), false],
    [
      'Use Template…: Problems',
      templateState({
        inputs: { feature: '', priority: 'high', api: 'api', web: 'docs', contract: true },
        preview: { ...demoTemplatePreview(), problems: [{ code: 'target_unknown', at: 'steps[1].target' }] },
        choices: { web: 'create' },
      }),
      false,
    ],
    [
      'Use Template…: Test Profile Problems',
      templateState({
        preview: {
          ...demoTemplatePreview(),
          problems: [{ code: 'validation_profile_unknown', at: 'steps[0].validation[0]' }],
        },
      }),
      false,
    ],
    [
      'Use Template…: Test Profiles Not Checked',
      templateState({
        preview: {
          ...demoTemplatePreview(),
          targets: demoTemplatePreview().targets.map((target) =>
            target.target_id === 'api' ? { ...target, profiles: 'unverified' as const } : target,
          ),
        },
      }),
      false,
    ],
    ['Use Template…: Applied', templateState({ mode: 'result', result: demoTemplateApply('planning') }), false],
    [
      'Use Template…: Waiting for the Planner',
      templateState({ mode: 'result', result: demoTemplateApply('waiting_planner') }),
      false,
    ],
    [
      'Save a Goal as Template…',
      templateState({
        mode: 'save',
        save: { goal: 'ticket-LCH-1', name: 'Rate limits again', description: '', targetsAsInputs: true },
      }),
      false,
    ],
    [
      'Use Template…: No Templates Yet',
      templateState({ templates: [], selected: undefined, preview: undefined }),
      false,
    ],
    ['Use Template…: Loading', templateState({ loading: true, templates: [], selected: undefined }), false],
    ['Use Template…: Phone', templateState({ detailActive: true }), true],
  ];
  return (
    <>
      {examples.map(([label, state, compact]) => (
        <CatalogExample label={label} viewport={{ width: compact ? 'compact' : 'full' }}>
          <OrchestrationTemplateDialog state={state} compact={compact} embedded />
        </CatalogExample>
      ))}
    </>
  );
}

/** The inspector of the selected card, in the state the settings choose. */
export function OrchestrationInspectorDemo() {
  const timeline = runningTimeline(),
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
              demoCiGate(),
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
            // HS2-65AD6J: the project's integrator for a card with a run.
            integrator: demoIntegrator(orchestrationDemoSettings.integrator.value),
            // HS2-9FS7YX: what the scheduler says of API-12 while it holds it back.
            held:
              orchestrationDemoSettings.selected.value === 'step-3' &&
              orchestrationDemoSettings.runCondition.value === 'held'
                ? {
                    standing: 'not_placed',
                    reason: 'never_joined',
                    detail: 'replacements_exhausted',
                    retry_at_ms: DEMO_NOW + 58 * 60_000,
                  }
                : undefined,
            // HS2-77SRHC, HS2-5GRXFN: DOC-9 is ready and waits for the VM someone holds.
            explanation:
              orchestrationDemoSettings.selected.value !== 'step-8'
                ? undefined
                : orchestrationDemoSettings.docWait.value === 'template_plan_not_accepted'
                  ? { standing: 'not_placed', reason: 'template_plan_not_accepted' }
                  : orchestrationDemoSettings.docWait.value === 'tool_signed_out'
                    ? {
                        // HS2-ZRPZJJ: Claude is signed out on this machine and on API's Build host.
                        standing: 'not_placed',
                        reason: 'no_tool_available',
                        detail: DEMO_SIGNED_OUT,
                        sites: [{ target_id: 'api', reason: 'no_tool_available', detail: DEMO_SIGNED_OUT }],
                      }
                    : orchestrationDemoSettings.docWait.value !== 'resource_held'
                      ? {
                          // HS2-747WQA: a pattern-only Models list on both of Docs's sites resolves to no model.
                          standing: 'not_placed',
                          reason: 'no_tool_available',
                          detail: orchestrationDemoSettings.docWait.value,
                          retry_at_ms: timeline.now_ms + 10 * 60_000,
                          sites: [{ target_id: 'api', reason: 'no_tool_available', detail: 'model_unresolved' }],
                        }
                      : {
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
                        },
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
              // HS2-7NJN7S: a gate event says which gate and the state it entered.
              {
                at_ms: timeline.now_ms - 60_000,
                kind: 'gate',
                gate_state: 'pending',
                gate_kind: 'ci',
                source: 'target',
              },
              // HS2-N9BFHW: a reconciliation reads its disposition in words.
              { at_ms: timeline.now_ms - 120_000, kind: 'reconciled', reason: 'continue', source: 'orchestrator' },
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
          overview={{
            decisions: demoDecisions(),
            contention: demoContention(),
            summary: demoSummary(),
            roles: demoRoles(),
            // HS2-X3FZJE: the Triggers row, with the next scheduled run.
            triggers: demoTriggers(),
          }}
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
      <Select name="integrator" label="Integrator" value={settings.integrator.value} choices={INTEGRATOR} />
      <Select name="doc-wait" label="DOC-9 Waits For" value={settings.docWait.value} choices={DOC_WAITS} />
      <Select name="ci-gate" label="API-12’s CI Gate" value={settings.ciGate.value} choices={CI_GATES} />
      <Select name="run-condition" label="API-12’s Run" value={settings.runCondition.value} choices={RUN_CONDITIONS} />
      <Select name="done-ci" label="Done API-11’s Gates" value={settings.doneCi.value} choices={DONE_CI} />
      <Checkbox name="busy" checked={settings.busy.value} label="Change in Progress" />
      <Checkbox name="default-goal" checked={settings.defaultGoal.value} label="No Goal Ticket Open" />
      <Checkbox name="auto-placement" checked={settings.autoPlacement.value} label="Placement On by Default" />
      <Checkbox name="policy-absent" checked={settings.policyAbsent.value} label="No Policy Saved" />
      <Checkbox name="never-started" checked={settings.neverStarted.value} label="Never Started" />
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
      if (name === 'integrator') settings.integrator.value = control.value;
      if (name === 'doc-wait') settings.docWait.value = control.value;
      if (name === 'ci-gate') settings.ciGate.value = control.value;
      if (name === 'done-ci') settings.doneCi.value = control.value;
      if (name === 'run-condition') settings.runCondition.value = control.value;
      if (name === 'busy') settings.busy.value = control.checked;
      if (name === 'default-goal') settings.defaultGoal.value = control.checked;
      if (name === 'auto-placement') settings.autoPlacement.value = control.checked;
      if (name === 'policy-absent') settings.policyAbsent.value = control.checked;
      if (name === 'never-started') settings.neverStarted.value = control.checked;
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
      updatePlan: () => {
        settings.planning.value = 'updating';
      },
      cancelPlanUpdate: () => {
        settings.planning.value = '';
      },
      answerQuestion: () => undefined,
      showPlanner: () => undefined,
      chooseProjects: () => undefined,
      runControl: () => undefined,
      showRequest: () => undefined,
      openGoal: () => undefined,
      acceptPlan: () => undefined,
      acceptAllPlans: () => undefined,
      runValidation: () => undefined,
      linkGate: () => undefined,
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

/** HS2-KXKA7X: the Triggers… dialog the header opens, in place, in each of its states. */
function triggerState(change: Partial<OrchestrationTriggerDialogState> = {}): OrchestrationTriggerDialogState {
  const triggers = demoTriggers();
  return {
    loading: false,
    triggers,
    selected: triggers[1].trigger_id,
    mode: 'detail',
    detailActive: false,
    form: newTriggerForm({ tool: 'claude', target: 'api' }),
    fixedInputs: {},
    templates: demoTemplates(),
    projects: [
      { target_id: 'api', label: 'API · Build host' },
      { target_id: 'web', label: 'Web · This computer' },
      { target_id: 'docs', label: 'Docs · This computer' },
    ],
    ownerName: 'Launch',
    review: demoTriggerReview(),
    reviewing: false,
    busy: false,
    now: DEMO_NOW,
    session: 1,
    ...change,
  };
}

function OrchestrationTriggerDialogDemo() {
  const triggers = demoTriggers();
  const setup = {
    ...newTriggerForm({ tool: 'claude', target: 'api' }),
    name: 'Repair API main CI',
    when: 'event' as const,
    event: 'branch_ci_failed' as const,
    repository: 'small/api',
    workflow: 'ci.yml',
    template: demoTemplates()[0].template_id,
    eventInputs: { feature: 'commit' as const },
  };
  // HS2-SEEMXR: a schedule trigger offers every recurrence field the command-schedule dialog has.
  const base = newTriggerForm({ tool: 'claude', target: 'api' });
  const scheduled = {
    ...base,
    name: 'Dependency bump on weekdays',
    placement: 'start' as const,
    schedule: {
      ...base.schedule,
      repeats: 'weekly' as const,
      startDate: '2026-11-02',
      weekdays: [1, 3, 5],
      time: '06:30',
      timeZone: 'UTC',
      endDate: '2027-03-01',
      excludeDates: '2026-12-25, 2027-01-01',
    },
  };
  const examples: Array<[string, OrchestrationTriggerDialogState, boolean]> = [
    // HS2-40GPZK: fixed values in Use Template…'s typed controls (a priority, projects and a checkbox).
    [
      'Triggers…: New Trigger',
      triggerState({
        mode: 'setup',
        form: setup,
        detailActive: true,
        fixedInputs: { priority: 'low', web: 'api', contract: false },
      }),
      false,
    ],
    [
      'Triggers…: New Schedule Trigger',
      triggerState({ mode: 'setup', form: scheduled, detailActive: true, review: undefined }),
      false,
    ],
    ['Triggers…: On', triggerState({ selected: triggers[0].trigger_id }), false],
    ['Triggers…: Dry Run', triggerState(), false],
    [
      'Triggers…: Overlapping Run Skipped',
      triggerState({
        triggers: [demoOverlapTrigger(), ...triggers.slice(1)],
        selected: triggers[0].trigger_id,
      }),
      false,
    ],
    ['Triggers…: Needs Review', triggerState({ selected: triggers[3].trigger_id }), false],
    ['Triggers…: Paused', triggerState({ selected: triggers[2].trigger_id }), false],
    ['Triggers…: No Triggers Yet', triggerState({ triggers: [], selected: undefined, review: undefined }), false],
    ['Triggers…: Loading', triggerState({ loading: true, triggers: [], selected: undefined }), false],
    ['Triggers…: Phone', triggerState({ detailActive: true }), true],
  ];
  return (
    <>
      {examples.map(([label, state, compact]) => (
        <CatalogExample label={label} viewport={{ width: compact ? 'compact' : 'full' }}>
          <OrchestrationTriggerDialog state={state} compact={compact} embedded />
        </CatalogExample>
      ))}
    </>
  );
}
