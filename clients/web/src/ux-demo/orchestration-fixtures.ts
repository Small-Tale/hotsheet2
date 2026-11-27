/**
 * HS2-J89JC7: deterministic, synthetic orchestration timelines for the UX catalog. An orchestration
 * project "Launch" plans work in "API" (remote, on "Build host"), "Web" and "Docs" (on this machine).
 * Every name, id and time is invented; nothing is read from a server or a real project.
 */
import type { TimelineHomeOutage } from '../orchestration-home-outage';
import type { CostTotals, ProgressSummaryView } from '../orchestration-progress';
import type { RoleStandingView, SchedulerDecisionPage } from '../orchestration-scheduler';
import type { PlanTemplate, TemplateApply, TemplatePreview } from '../orchestration-templates';
import type {
  OrchestrationTimeline,
  TimelineItem,
  TimelinePhase,
  TimelineTarget,
  TimelineWave,
} from '../orchestration-timeline';
import { parseTriggerList, parseTriggerReview, type TriggerReview, type TriggerView } from '../orchestration-triggers';

/** 14:00 on a fixed day, so captures and tests read the same times. */
export const DEMO_NOW = Date.UTC(2026, 10, 14, 14, 0);
const MINUTE = 60_000;

export const DEMO_TARGETS: TimelineTarget[] = [
  { target_id: 'api', label: 'API', location: 'remote', machine_label: 'Build host', checkout_id: 'api-0123456789ab' },
  { target_id: 'web', label: 'Web', location: 'local', checkout_id: 'web-0123456789ab' },
  { target_id: 'docs', label: 'Docs', location: 'local', checkout_id: 'docs-0123456789ab' },
];

const GOALS = {
  'LCH-1': 'Ship single sign-on',
  'LCH-2': 'Publish export format version 2',
  'LCH-3': 'Upgrade the build toolchain',
} as const;

function card(
  step: number,
  target: 'api' | 'web' | 'docs',
  slug: string,
  title: string,
  goal: keyof typeof GOALS,
  rest: Partial<TimelineItem> & Pick<TimelineItem, 'state' | 'level'>,
): TimelineItem {
  const project = DEMO_TARGETS.find((item) => item.target_id === target)!;
  return {
    step_id: `step-${step}`,
    goal: { connection_id: 'launch-tickets', native_id: `goal-${goal}` },
    goal_slug: goal,
    goal_title: GOALS[goal],
    target: { target_id: target, connection_id: `${target}-tickets`, native_id: `ticket-${slug}` },
    slug,
    title,
    status: 'not_started',
    priority: 'default',
    category: 'feature',
    site_id: project.target_id,
    project_label: project.label,
    location: project.location,
    machine_label: project.machine_label,
    tool: 'claude',
    ...rest,
  };
}

const run = (id: string, started: number, extra: Partial<NonNullable<TimelineItem['run']>> = {}) => ({
  run_id: id,
  state: 'dispatched',
  worker_id: 'worker-1',
  lease_expires_at_ms: DEMO_NOW + 22 * MINUTE,
  paused: false,
  cancel_requested: false,
  last_confirmed_at_ms: DEMO_NOW - MINUTE,
  created_at_ms: started,
  ...extra,
});

/** The running timeline of the signed-off design (HS2-HSMH2Q Design v3). */
function runningWaves(): TimelineWave[] {
  return [
    {
      kind: 'done',
      ordinal: 0,
      items: [
        card(1, 'api', 'API-11', 'Store single sign-on client settings', 'LCH-1', {
          state: 'done',
          level: 'finished',
          status: 'completed',
          started_at_ms: DEMO_NOW - 240 * MINUTE,
          finished_at_ms: DEMO_NOW - 160 * MINUTE,
          run: run('run-1', DEMO_NOW - 240 * MINUTE, { state: 'completed', finished_at_ms: DEMO_NOW - 160 * MINUTE }),
        }),
        card(2, 'api', 'API-14', 'Accept export schema version 2', 'LCH-2', {
          state: 'done',
          level: 'finished',
          status: 'completed',
          tool: 'codex',
          started_at_ms: DEMO_NOW - 200 * MINUTE,
          finished_at_ms: DEMO_NOW - 75 * MINUTE,
          run: run('run-2', DEMO_NOW - 200 * MINUTE, { state: 'completed', finished_at_ms: DEMO_NOW - 75 * MINUTE }),
        }),
      ],
    },
    {
      kind: 'working_now',
      ordinal: 0,
      estimated_start_ms: DEMO_NOW - 8 * MINUTE,
      estimate: 'started',
      items: [
        card(3, 'api', 'API-12', 'Add the single sign-on token endpoint', 'LCH-1', {
          state: 'running',
          level: 'in_progress',
          status: 'started',
          priority: 'high',
          session_id: 'session-3',
          session_state: 'running',
          started_at_ms: DEMO_NOW - 5 * MINUTE,
          run: run('run-3', DEMO_NOW - 5 * MINUTE),
        }),
        card(4, 'web', 'WEB-33', 'Read export schema version 2', 'LCH-2', {
          state: 'running',
          level: 'in_progress',
          status: 'started',
          tool: 'codex',
          session_id: 'session-4',
          session_state: 'running',
          started_at_ms: DEMO_NOW - 8 * MINUTE,
          run: run('run-4', DEMO_NOW - 8 * MINUTE),
        }),
        card(5, 'docs', 'DOC-7', 'Document export schema version 2', 'LCH-2', {
          state: 'provisional',
          level: 'in_progress',
          status: 'started',
          category: 'task',
          session_id: 'session-5',
          session_state: 'running',
          waits_on: [
            {
              target_id: 'web',
              connection_id: 'web-tickets',
              native_id: 'ticket-WEB-33',
              slug: 'WEB-33',
              step_id: 'step-4',
              reason: 'gate_pending',
            },
          ],
          started_at_ms: DEMO_NOW - 2 * MINUTE,
          run: run('run-5', DEMO_NOW - 2 * MINUTE),
        }),
      ],
    },
    {
      kind: 'next',
      ordinal: 0,
      estimated_start_ms: DEMO_NOW + 60 * MINUTE,
      estimate: 'measured',
      items: [
        card(6, 'api', 'API-15', 'Move API to the new toolchain', 'LCH-3', {
          state: 'waits_for_permission',
          level: 'confirmed',
          category: 'task',
          tool: 'codex',
          estimated_start_ms: DEMO_NOW + 60 * MINUTE,
          permission: {
            kind: 'permission',
            request_id: 'request-6',
            action: 'session_start',
            requested_at_ms: DEMO_NOW + 6 * MINUTE,
          },
        }),
        card(7, 'web', 'WEB-30', 'Add the single sign-on login page', 'LCH-1', {
          state: 'waits_for_decision',
          level: 'provisional',
          priority: 'high',
          estimated_start_ms: DEMO_NOW + 70 * MINUTE,
          decision: { note_id: 'note-7' },
          waits_on: [
            {
              target_id: 'api',
              connection_id: 'api-tickets',
              native_id: 'ticket-API-12',
              slug: 'API-12',
              step_id: 'step-3',
              reason: 'gate_pending',
            },
          ],
        }),
        card(8, 'docs', 'DOC-9', 'Describe the single sign-on settings', 'LCH-1', {
          state: 'ready',
          level: 'confirmed',
          category: 'task',
          estimated_start_ms: DEMO_NOW + 60 * MINUTE,
          placement: { at_ms: DEMO_NOW, decision: 'unplaced', reason: 'target_cap', tool: 'claude' },
        }),
      ],
    },
    {
      kind: 'then',
      ordinal: 0,
      estimated_start_ms: DEMO_NOW + 150 * MINUTE,
      estimate: 'measured',
      items: [
        card(9, 'web', 'WEB-35', 'Move Web to the new toolchain', 'LCH-3', {
          state: 'waits_for_prerequisite',
          level: 'blocked',
          category: 'task',
          estimated_start_ms: DEMO_NOW + 150 * MINUTE,
          reasons: ['prerequisite_not_finished'],
          waits_on: [
            {
              target_id: 'api',
              connection_id: 'api-tickets',
              native_id: 'ticket-API-15',
              slug: 'API-15',
              step_id: 'step-6',
              reason: 'prerequisite_not_finished',
            },
          ],
        }),
      ],
    },
    {
      kind: 'later',
      ordinal: 0,
      estimated_start_ms: DEMO_NOW + 240 * MINUTE,
      estimate: 'measured',
      items: [
        card(10, 'docs', 'DOC-11', 'Announce the new build toolchain', 'LCH-3', {
          state: 'waits_for_prerequisite',
          level: 'blocked',
          category: 'task',
          estimated_start_ms: DEMO_NOW + 240 * MINUTE,
          waits_on: [
            {
              target_id: 'web',
              connection_id: 'web-tickets',
              native_id: 'ticket-WEB-35',
              slug: 'WEB-35',
              step_id: 'step-9',
              reason: 'prerequisite_not_finished',
            },
          ],
        }),
      ],
    },
  ];
}

/** The same plan before kick-off: waves First, Second, … from the dependency order. */
function proposedWaves(): TimelineWave[] {
  const all = runningWaves().flatMap((wave) => wave.items),
    planned = (ordinal: number, steps: number[]): TimelineWave => ({
      kind: 'planned',
      ordinal,
      estimated_start_ms: DEMO_NOW + ordinal * 90 * MINUTE,
      estimate: 'default',
      items: steps.map((step) => {
        const item = all.find((candidate) => candidate.step_id === `step-${step}`)!;
        return {
          ...item,
          state: item.waits_on ? 'waits_for_prerequisite' : 'ready',
          level: item.waits_on ? 'blocked' : 'confirmed',
          status: 'not_started',
          run: undefined,
          session_id: undefined,
          session_state: undefined,
          permission: undefined,
          decision: undefined,
          started_at_ms: undefined,
          finished_at_ms: undefined,
          estimated_start_ms: DEMO_NOW + ordinal * 90 * MINUTE,
        };
      }),
    });
  return [planned(0, [1, 2, 3, 4, 6, 8]), planned(1, [5, 7, 9]), planned(2, [10])];
}

/** A synthetic timeline in one phase. */
export function demoTimeline(phase: TimelinePhase): OrchestrationTimeline {
  const waves =
    phase === 'running' || phase === 'paused' ? runningWaves() : phase === 'proposed' ? proposedWaves() : [];
  const counts: Record<string, number> = {};
  for (const item of waves.flatMap((wave) => wave.items)) counts[item.state] = (counts[item.state] ?? 0) + 1;
  return {
    version: `demo-${phase}`,
    now_ms: DEMO_NOW,
    declared: true,
    phase,
    automatic: phase === 'running',
    kick_off: {
      can_start: phase === 'proposed' || phase === 'paused',
      start_blocked:
        phase === 'running'
          ? 'running'
          : phase === 'no_projects'
            ? 'no_projects'
            : phase === 'no_plan'
              ? 'no_plan'
              : undefined,
      can_pause: phase === 'running',
      policy_revision: 'demo-revision',
    },
    running_since_ms: phase === 'running' || phase === 'paused' ? DEMO_NOW - 118 * MINUTE : undefined,
    sessions: phase === 'running' ? 3 : phase === 'paused' ? 2 : 0,
    targets: phase === 'no_projects' ? [] : DEMO_TARGETS,
    waves,
    counts,
    wave_minutes: 90,
    estimate: phase === 'running' ? 'measured' : 'default',
  };
}

/**
 * HS2-36XDDX: a ready card held for its template's accept: the step a template's goal planned, which placement
 * starts once someone accepts the proposed plan.
 */
export function demoTemplateHeld(item: TimelineItem): TimelineItem {
  return {
    ...item,
    state: 'waits_for_accept',
    placement: undefined,
    accept: { operation_id: 'tpl-docs-1', template_name: 'Document a feature' },
  };
}

/** A fixed 24-hour clock for demo captures, independent of the viewer's locale and time zone. */
export const demoClock = (ms: number) => {
  const date = new Date(ms);
  return `${String(date.getUTCHours()).padStart(2, '0')}:${String(date.getUTCMinutes()).padStart(2, '0')}`;
};

/** HS2-77SRHC: the newest page of a synthetic decision log for the running timeline. */
export function demoDecisions(): SchedulerDecisionPage {
  const at = (minutes: number) => DEMO_NOW - minutes * MINUTE;
  const decision = (
    seq: number,
    minutes: number,
    kind: string,
    rest: Partial<SchedulerDecisionPage['entries'][number]> = {},
  ) => ({
    seq,
    at_ms: at(minutes),
    tick_id: `tick-${Math.floor(seq / 10)}`,
    mode: 'applied',
    kind,
    ...rest,
  });
  return {
    cursor: 34,
    more: true,
    entries: [
      decision(42, 2, 'tick', { tick_id: 'tick-4' }),
      decision(41, 2, 'start', { tick_id: 'tick-4', step_id: 'step-3', tool: 'claude' }),
      decision(40, 2, 'unplaced', {
        tick_id: 'tick-4',
        step_id: 'step-8',
        reason: 'resource_held',
        detail: 'vm/macos-vm',
      }),
      decision(39, 21, 'start', {
        tick_id: 'tick-3',
        step_id: 'step-6',
        tool: 'codex',
        outcome: 'permission_required',
      }),
      decision(38, 21, 'tick', { tick_id: 'tick-3' }),
      decision(37, 40, 'keep', { tick_id: 'tick-3', step_id: 'step-4', reason: 'mid_turn' }),
      decision(36, 60, 'wind_down', { tick_id: 'tick-3', reason: 'no_ready_work' }),
      // HS2-2YCHRC: a role session's start names its role and project, not a step.
      decision(35, 60, 'start', {
        tick_id: 'tick-3',
        target_id: 'api',
        tool: 'claude',
        reason: 'integrator_started',
        detail: 'handoff_pending',
      }),
    ],
  };
}

/** HS2-5GRXFN: the scheduler plan's contention for the running timeline: the VM DOC-9 waits for. */
export function demoContention() {
  return [
    {
      namespace: 'vm',
      resource: 'macos-vm',
      state: 'held',
      expires_at_ms: DEMO_NOW + 40 * MINUTE,
      waiting: ['step-8'],
    },
  ];
}

/**
 * HS2-65AD6J: the API project's integrator in each standing `scheduler explain` gives a running card, keyed by
 * the demo's Integrator setting. Unavailable is `integrator_unavailable` after its replacements were used up.
 */
export function demoIntegrator(state: string): RoleStandingView | undefined {
  const api = { target_id: 'api', role: 'integrator' };
  switch (state) {
    case 'start':
      return { ...api, standing: 'start', reason: 'integrator_started', detail: 'handoff_pending' };
    case 'keep':
      return {
        ...api,
        standing: 'keep',
        reason: 'integrator_kept',
        detail: 'ci_pending',
        session_id: 'sched-role-integrator-api',
      };
    case 'wind_down':
      return { ...api, standing: 'wind_down', reason: 'integrator_wound_down', detail: 'idle' };
    case 'not_started':
      return {
        ...api,
        standing: 'not_started',
        reason: 'integrator_not_started',
        detail: 'target_cap',
        retry_at_ms: DEMO_NOW + 10 * MINUTE,
      };
    case 'no_model':
      // HS2-747WQA: the project's Models pattern resolves to no model its machine reports.
      return {
        ...api,
        standing: 'not_started',
        reason: 'integrator_not_started',
        detail: 'model_unresolved',
        retry_at_ms: DEMO_NOW + 10 * MINUTE,
      };
    case 'opt_out':
      return { ...api, standing: 'not_started', reason: 'integrator_not_started', detail: 'policy_opt_out' };
    case 'signed_out':
      // HS2-GK6HC8: every AI tool it may run is signed out on API's machine; it waits for the sign-in.
      return { ...api, standing: 'not_started', reason: 'integrator_not_started', detail: 'tool_signed_out:claude' };
    case 'unavailable':
      return {
        ...api,
        standing: 'not_started',
        reason: 'integrator_unavailable',
        detail: 'replacements_exhausted',
        retry_at_ms: DEMO_NOW + 60 * MINUTE,
      };
    case 'none':
      return { ...api, standing: 'none' };
    default:
      return undefined;
  }
}

/**
 * HS2-65AD6J: the overview's coordination sessions: API integrates, Web's dispatchers never joined.
 * HS2-GK6HC8: API's dispatcher waits for Claude to be signed in on its machine.
 */
export function demoRoles(): RoleStandingView[] {
  return [
    { target_id: 'api', role: 'integrator', standing: 'keep', reason: 'integrator_kept', detail: 'queue' },
    {
      target_id: 'api',
      role: 'dispatcher',
      standing: 'not_started',
      reason: 'dispatcher_not_started',
      detail: 'tool_signed_out:claude',
    },
    {
      target_id: 'web',
      role: 'dispatcher',
      standing: 'not_started',
      reason: 'dispatcher_unavailable',
      detail: 'replacements_exhausted',
      retry_at_ms: DEMO_NOW + 60 * MINUTE,
    },
    {
      target_id: 'docs',
      role: 'integrator',
      standing: 'not_started',
      reason: 'integrator_not_started',
      detail: 'policy_opt_out',
    },
  ];
}

/** HS2-XJEH08: progress and cost figures for the running timeline; Docs reports no cost. */
export function demoSummary(): ProgressSummaryView {
  const cost = (known_usd: number, complete: boolean, items_known: number, items_unknown: number): CostTotals => ({
    known_usd,
    complete,
    items_known,
    items_unknown,
    unpriced_events: 0,
    unknown_reasons: items_unknown > 0 ? { no_usage_recorded: items_unknown } : {},
  });
  return {
    totals: {
      lead_time: { known: 2, unknown: 3, total_ms: 2 * 104 * MINUTE, unknown_reasons: { run_unfinished: 3 } },
      cost: cost(12.4, false, 3, 1),
    },
    targets: {
      api: { cost: cost(8.1, true, 2, 0) },
      web: { cost: cost(4.3, true, 1, 0) },
      docs: { cost: cost(0, false, 0, 1) },
    },
  };
}

/** HS2-EX594V: two projects whose home sites cannot coordinate them: one unreachable, one disabled. */
export function demoHomeOutages(): TimelineHomeOutage[] {
  return [
    {
      home_target_id: 'api',
      sites: ['web'],
      since_ms: DEMO_NOW - 12 * MINUTE,
      reason: 'unreachable',
      detail: 'unreachable',
      announced: true,
    },
    { home_target_id: 'docs', sites: ['web', 'api'], since_ms: DEMO_NOW - 3 * MINUTE, reason: 'disabled' },
  ];
}

/** HS2-A4RVYA: the design's templates, synthetic. */
export function demoTemplates(): PlanTemplate[] {
  return [
    {
      template_id: '01JTEMPLATE00000000000FEAT',
      name: 'Feature across API and Web',
      description: 'API change, then the Web change on top of its green CI, then a contract check.',
      state: 'active',
      version: 3,
      inputs: [
        { key: 'feature', label: 'Feature', kind: 'text', required: true, choices: [] },
        { key: 'priority', label: 'Priority', kind: 'priority', required: false, default: 'high', choices: [] },
        { key: 'api', label: 'API Project', kind: 'target', required: true, default: 'api', choices: [] },
        { key: 'web', label: 'Web Project', kind: 'target', required: true, default: 'web', choices: [] },
        {
          key: 'contract',
          label: 'Run the API Contract Check',
          kind: 'boolean',
          required: false,
          default: true,
          choices: [],
          hint: 'Uses the API project’s own contract-check profile',
        },
      ],
      goal_title: '{{feature}}: API and Web',
      wait_for_accept: true,
      steps: [
        {
          key: 'api',
          target: '{{api}}',
          title: 'Add {{feature}} headers',
          ticket: 'search',
          after: [],
          validation: [],
        },
        {
          key: 'web',
          target: '{{web}}',
          title: 'Show {{feature}} errors',
          ticket: 'search',
          after: [{ step: 'api', gates: 'ci' }],
          validation: [],
        },
        {
          key: 'docs',
          target: '{{web}}',
          title: 'Document {{feature}}',
          ticket: 'create',
          after: [{ step: 'web' }],
          validation: [],
        },
      ],
    },
    {
      template_id: '01JTEMPLATE00000000000REPR',
      name: 'Repair failing CI',
      description: 'One repair ticket in the project whose CI failed, worked first.',
      state: 'active',
      version: 1,
      inputs: [{ key: 'project', label: 'Project', kind: 'target', required: true, choices: [] }],
      goal_title: 'Repair CI',
      wait_for_accept: true,
      steps: [
        {
          key: 'fix',
          target: '{{project}}',
          title: 'Repair the failing CI',
          ticket: 'search',
          after: [],
          validation: [],
        },
      ],
    },
    {
      template_id: '01JTEMPLATE00000000000BUMP',
      name: 'Dependency bump in every project',
      description: 'One upgrade ticket per chosen project, each validated by its own test profile.',
      state: 'active',
      version: 2,
      inputs: [],
      goal_title: 'Bump dependencies',
      // HS2-36XDDX: a routine bump starts without waiting for an accept.
      wait_for_accept: false,
      steps: [
        { key: 'bump', target: 'api', title: 'Bump dependencies', ticket: 'search', after: [], validation: ['unit'] },
      ],
    },
  ];
}

/** HS2-A4RVYA: the pre-run summary of the first demo template with the inputs the design shows. */
export function demoTemplatePreview(): TemplatePreview {
  return {
    template_id: '01JTEMPLATE00000000000FEAT',
    problems: [],
    goal_title: 'Rate limits: API and Web',
    steps: [
      {
        key: 'api',
        target_id: 'api',
        title: 'Add rate-limit headers',
        ticket: 'search',
        after: [],
        priority: 'high',
        tool: 'claude',
        validation: ['contract-check'],
      },
      {
        key: 'web',
        target_id: 'web',
        title: 'Show rate-limit errors',
        ticket: 'search',
        after: [{ step: 'api', gates: 'ci' }],
        validation: [],
      },
      {
        key: 'docs',
        target_id: 'web',
        title: 'Document rate limits',
        ticket: 'create',
        after: [{ step: 'web' }],
        validation: [],
      },
    ],
    skipped: [],
    targets: [
      {
        target_id: 'api',
        label: 'API',
        declared: true,
        steps: ['api'],
        apply: 'person',
        ticket_writes: 'remembered',
        profiles: 'checked',
      },
      {
        target_id: 'web',
        label: 'Web',
        declared: true,
        steps: ['web', 'docs'],
        apply: 'person',
        ticket_writes: 'asks',
      },
    ],
    planning: 'propose',
    placement_automatic: true,
    wait_for_accept: true,
  };
}

/** HS2-A4RVYA: an apply's outcome, in each state the dialog shows. */
export function demoTemplateApply(state: TemplateApply['state']): TemplateApply {
  return {
    operation_id: 'tpl-4f1c09b2e7a5',
    template_id: '01JTEMPLATE00000000000FEAT',
    template_name: 'Feature across API and Web',
    template_version: 3,
    state,
    ...(state === 'waiting_permission' || state === 'denied'
      ? {}
      : { goal: { connection_id: 'launch-tickets', native_id: 'ticket-LCH-7', slug: 'LCH-7', created: true } }),
    ...(state === 'planning' ? { planning: 'propose' as const } : {}),
    ...(state === 'waiting_planner' ? { reason: 'plan_update_planner_busy' } : {}),
    permission_requests: state === 'waiting_permission' ? ['request-1'] : [],
    outcome: 'applied',
  };
}

/** HS2-KXKA7X: the design's triggers, synthetic, as the server answers `triggers/list`. */
export function demoTriggers(): TriggerView[] {
  const HOUR = 60 * MINUTE;
  const at = (hoursAgo: number) => DEMO_NOW - hoursAgo * HOUR;
  const raw = [
    {
      trigger_id: '01JTRIGGER0000000000000BMP',
      name: 'Dependency bump',
      source: {
        kind: 'schedule',
        schedule: {
          time_zone: 'UTC',
          local_time: '02:00',
          recurrence: { kind: 'hourly', start_date: '2026-11-01', every: 4 },
        },
      },
      action: { template: { template_id: '01JTEMPLATE00000000000FEAT', tool: 'claude' } },
      state: 'on',
      review: { role: 'human', actor_id: 'maintainer', at_ms: at(24 * 6) },
      history: [
        {
          delivery_key: 'schedule:a',
          kind: 'schedule',
          at_ms: at(40),
          outcome: 'applied',
          goal_slug: 'LCH-11',
          missed: 1,
        },
        { delivery_key: 'schedule:b', kind: 'schedule', at_ms: at(12), outcome: 'applied', goal_slug: 'LCH-12' },
      ],
      next_runs_ms: [DEMO_NOW + 2 * HOUR, DEMO_NOW + 6 * HOUR, DEMO_NOW + 10 * HOUR],
      template_name: 'Dependency bump in every project',
      template_version: 2,
      reviewed: true,
      running: false,
      updated_at_ms: at(24 * 6),
    },
    {
      trigger_id: '01JTRIGGER0000000000000REP',
      name: 'Repair API main CI',
      source: {
        kind: 'branch_ci_failed',
        target_id: 'api',
        watch: { repository: 'small/api', workflow: 'ci.yml', branch: 'main' },
      },
      action: {
        template: { template_id: '01JTEMPLATE00000000000FEAT', tool: 'claude', event_inputs: { feature: 'commit' } },
      },
      state: 'dry_run',
      history: [
        {
          delivery_key: 'ci:github-actions:9177:1',
          kind: 'ci_failed',
          at_ms: at(3),
          outcome: 'dry_run',
          evidence: { commit: 'a71e004aa', ci_run: '9177.1' },
          would: { goal_title: 'Repair API CI on a71e004', steps: 1, targets: ['api'], asks: ['api'] },
        },
        {
          delivery_key: 'ci:github-actions:9182:1',
          kind: 'ci_failed',
          at_ms: at(0.5),
          outcome: 'dry_run',
          evidence: { commit: '3f9c2a1bb', ci_run: '9182.1' },
          would: { goal_title: 'Repair API CI on 3f9c2a1', steps: 1, targets: ['api'], asks: ['api'] },
        },
        { delivery_key: 'ci:github-actions:9182:1', kind: 'ci_failed', at_ms: at(0.4), outcome: 'already_handled' },
      ],
      template_name: 'Repair failing CI',
      reviewed: false,
      running: false,
      updated_at_ms: at(5),
    },
    {
      trigger_id: '01JTRIGGER0000000000000WEB',
      name: 'Validate merged Web commits',
      source: {
        kind: 'commit_landed',
        target_id: 'web',
        watch: { repository: 'small/web', workflow: 'ci.yml', branch: 'main' },
      },
      action: { placement: 'start' },
      state: 'paused',
      paused_from: 'on',
      history: [],
      reviewed: true,
      running: false,
      updated_at_ms: at(30),
    },
    {
      trigger_id: '01JTRIGGER0000000000000DOC',
      name: 'Weekly docs sweep',
      source: {
        kind: 'schedule',
        schedule: {
          time_zone: 'UTC',
          local_time: '09:00',
          recurrence: { kind: 'weekly', start_date: '2026-11-02', every: 1, weekdays: [1] },
        },
      },
      action: { template: { template_id: '01JTEMPLATE00000000000FEAT', tool: 'codex' } },
      state: 'needs_review',
      history: [
        { delivery_key: 'review:3', kind: 'manual', at_ms: at(2), outcome: 'needs_review', reason: 'template_changed' },
      ],
      reviewed: false,
      running: false,
      updated_at_ms: at(2),
    },
  ];
  return parseTriggerList({ triggers: raw });
}

/** HS2-KXKA7X: the overlap state of the design: the 02:00 run still works, later slots were skipped. */
export function demoOverlapTrigger(): TriggerView {
  const [bump] = demoTriggers();
  const HOUR = 60 * MINUTE;
  return {
    ...bump,
    running: true,
    running_goal: 'LCH-12',
    history: [
      ...bump.history,
      {
        delivery_key: 'schedule:c',
        kind: 'schedule',
        at_ms: DEMO_NOW - 4 * HOUR,
        outcome: 'skipped_overlap',
        count: 2,
        evidence: {},
        missed: 0,
      },
    ],
  };
}

/** HS2-KXKA7X: the review of the CI repair trigger, as `triggers/review` answers it. */
export function demoTriggerReview(): TriggerReview {
  return parseTriggerReview({
    problems: [],
    scope: {
      watches: {
        kind: 'branch_ci_failed',
        target_id: 'api',
        label: 'API · Build host',
        branch: { repository: 'small/api', workflow: 'ci.yml', branch: 'main' },
      },
      projects: [{ target_id: 'api', label: 'API · Build host', declared: true, steps: 1 }],
      per_delivery: { goals: 1, steps: 1, goal_title: 'Repair API CI on <commit>' },
    },
    permissions: [
      { action: 'watch', target_id: 'api', need: 'not_needed' },
      { action: 'tickets_write', target_id: 'api', need: 'asks' },
      { action: 'session_start', target_id: 'api', need: 'remembered', tool: 'claude', rule_id: 'rule-1' },
      { action: 'push_deploy', need: 'unchanged' },
    ],
    next: { runs_ms: [], event: 'branch_ci_failed', overlap: 'skip' },
    digest: 'demo-digest',
    template_version: 3,
    turn_on: 'person',
  });
}
