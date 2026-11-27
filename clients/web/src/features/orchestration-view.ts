/**
 * HS2-J89JC7: the Orchestration view's state and side effects. One genuine long poll of the selected
 * project's timeline while the view is open (version cursor, bounded failure backoff, no interval);
 * otherwise one read per project to learn whether it is an orchestration project, so the view
 * switcher offers the tab. Selection, the inspector's run details, kick-off and run controls live
 * here; the shell renders from these signals.
 */
import { batch, computed, effect, type Signal, signal } from 'kerfjs';

import { Api, ApiHttpError } from '../api';
import {
  type BackoffDelay,
  describeRequestFailure,
  isTransientRequestFailure,
  retryTransient,
} from '../bounded-backoff';
import type { OrchestrationDecisionsDialogState } from '../components/orchestration-decisions-dialog';
import type { OrchestrationInspectorDetails } from '../components/orchestration-inspector';
import type { OrchestrationTemplateDialogState } from '../components/orchestration-template-dialog';
import type { OrchestrationProposeAvailability } from '../components/orchestration-timeline-view';
import type { WorkspaceViewMode } from '../components/workspace-controls';
import type { ConfirmationOptions } from '../confirmation';
import type { Project } from '../interactions/types';
import { parseBuiltAhead } from '../orchestration-built-ahead';
import { parsePlanHealth, PLAN_FAILURE_CODES, type PlanHealth } from '../orchestration-plan-health';
import { parseProgressSummary, type ProgressSummaryView } from '../orchestration-progress';
import { describeRefusal } from '../orchestration-refusals';
import { createRemoteLabelReader, type RemoteTicketLabel, withRemoteLabels } from '../orchestration-remote-labels';
import { parseContention, type ResourceContentionView } from '../orchestration-resources';
import {
  CI_WATCH_REASONS,
  parseRunReconciliation,
  relinkRequest,
  REVIEW_GATE_STATES,
} from '../orchestration-run-checks';
import {
  heldExplanation,
  parseDecisionPage,
  parseExplanation,
  parseIntegratorStanding,
  parseRoleStandings,
  type RoleStandingView,
  type SchedulerDecisionPage,
} from '../orchestration-scheduler';
import {
  initialInputs,
  inputsToSend,
  newApplyOperationId,
  parsePlanTemplate,
  parseTemplateApply,
  parseTemplateList,
  parseTemplatePreview,
  savableGoals,
  type SaveGoalForm,
  saveProblem,
  type StepTicketChoice,
} from '../orchestration-templates';
import type { OrchestrationTimeline } from '../orchestration-timeline';
import { findTimelineItem, startColumnId } from '../orchestration-timeline-board';
import {
  parseValidationOperation,
  parseValidationRunAnswer,
  parseValidationStep,
  type PendingOrchestratorAsk,
  type PendingValidation,
  recoveredValidations,
  stepsToRun,
  validationOperation,
  validationRunUnsettled,
} from '../orchestration-validation-steps';
import { collapsedTicketGroupKeys, toggleTicketGroupCollapsed } from '../ticket-group-state';
import type { TicketLinkMatch } from '../ticket-link-resolution';
import { startVersionedLongPoll } from '../versioned-long-poll';
import {
  isOrchestrationProjectViewMode,
  type OrchestrationProjectViewMode,
  projectArrivalView,
  rememberOrchestrationProjectView,
} from '../workspace-view-memory';

/** The collapse scope of the Orchestration view's project groups (HS2-WZ3JGZ storage). */
export const ORCHESTRATION_GROUP_SCOPE = 'orchestration';

/** The planning session behind Propose Plan (HS2-HCW7MD); the UX demo passes fixture state. */
export interface OrchestrationPlanner {
  availability(project: Project): OrchestrationProposeAvailability;
  /** HS2-A4RVYA: the AI tool a template's planner runs in, as Propose Plan would choose it. */
  choice?(project: Project): AiToolChoice | undefined;
  propose(project: Project): Promise<string>;
  /** HS2-190RDX: Update Plan, which re-plans beside the running plan; answers the toast. */
  update(project: Project): Promise<string>;
  /** HS2-190RDX: cancel the drafting plan update; answers the toast. */
  cancelUpdate(project: Project): Promise<string>;
}

/** The AI tool the person chose for this project (the Drive choice), with its model and effort. */
export interface AiToolChoice {
  tool: string;
  model?: string;
  effort?: string;
}

/** A stable, bounded actor id for this browser's person, so orchestration audits name who acted. */
export function orchestrationPersonId(storage: Pick<Storage, 'getItem' | 'setItem'> | undefined): string {
  const key = 'hotsheet.orchestrationPersonId';
  try {
    const saved = storage?.getItem(key);
    if (saved && /^[A-Za-z0-9._-]{1,160}$/.test(saved)) return saved;
  } catch {
    // Storage may be unavailable; a fresh id still names this session's person.
  }
  const id = `person-${crypto.randomUUID().replaceAll('-', '')}`;
  try {
    storage?.setItem(key, id);
  } catch {
    // Kept for this session only.
  }
  return id;
}

/**
 * HS2-5RYXNN: where planning stands, as the view shows it. `waiting` is a planner whose terminal runs
 * while its tool has not reported its session start: it may be waiting on a first-run question (trust,
 * an MCP or hook approval, an update notice) that only a person answers in its terminal tab.
 * HS2-FDS95D: `replannable` is a planner that finished its turn, so Propose Plan gives it a new proposal.
 * HS2-190RDX: `updating` while a plan update drafts beside the running plan.
 * HS2-N5BDE4: `asked` while a goal ticket holds the planner's unanswered question.
 */
export function planningPhase(view: Record<string, unknown> | undefined): string | undefined {
  const state = typeof view?.state === 'string' ? view.state : undefined;
  const session = view?.session as Record<string, unknown> | undefined;
  const asked = planningQuestion(view) !== undefined;
  if (state === 'ended' && asked) return 'asked';
  if (state !== 'planning') return state;
  if (session?.tool_ready === false) return 'waiting';
  if (asked) return 'asked';
  const update = view?.update as Record<string, unknown> | undefined;
  if (update?.state === 'drafting') return 'updating';
  return view?.replannable === true ? 'replannable' : state;
}

/** HS2-FA8G9J: where the planning session's terminal runs: its checkout (the drawer's tab) and terminal id. */
export interface PlannerTerminal {
  checkout_id: string;
  session_id: string;
}

/** HS2-FA8G9J: the planner's terminal from `plan show` (`session.session`), when it names one. */
export function plannerTerminalOf(view: Record<string, unknown> | undefined): PlannerTerminal | undefined {
  const hosted = (view?.session as Record<string, unknown> | undefined)?.session as Record<string, unknown> | undefined;
  return typeof hosted?.checkout_id === 'string' && typeof hosted.session_id === 'string'
    ? { checkout_id: hosted.checkout_id, session_id: hosted.session_id }
    : undefined;
}

/**
 * HS2-YSRT6Y: whether `plan show` says no explicit goal ticket is open, so planning works through all open
 * tickets of the orchestrated projects.
 */
export function planningDefaultGoal(view: Record<string, unknown> | undefined): boolean {
  return view?.default_goal === true;
}

/** HS2-N5BDE4: the goal ticket whose question the planner asked the person, from `plan show`. */
export interface PlanningQuestion {
  connection_id: string;
  native_id: string;
  slug: string;
  note_id?: string;
}

export function planningQuestion(view: Record<string, unknown> | undefined): PlanningQuestion | undefined {
  const questions = Array.isArray(view?.questions) ? (view.questions as Record<string, unknown>[]) : [];
  const first = questions.find(
    (question) => typeof question.native_id === 'string' && typeof question.connection_id === 'string',
  );
  if (!first) return undefined;
  return {
    connection_id: first.connection_id as string,
    native_id: first.native_id as string,
    slug: typeof first.slug === 'string' ? first.slug : (first.native_id as string),
    note_id: typeof first.question === 'string' ? first.question : undefined,
  };
}

/**
 * A planning refusal in words (HS2-8Z9VHH): every code, and a refused admission with its limit and
 * observed value, reads through the shared refusal map; the code stays as secondary detail.
 */
function planningRefusal(reason: unknown): never {
  if (reason instanceof ApiHttpError && (reason.code || reason.admission))
    throw new Error(describeRefusal(reason), { cause: reason });
  throw reason;
}

const CHOOSE_TOOL = 'Choose an AI tool for this project in Settings first';
// HS2-N3HWP9: with no tool chosen the planner runs Auto, in an installed tool; this is the case where
// none is installed, so choosing one in Settings would not help.
const NO_TOOL = 'Install and sign in to an AI tool on this machine first';

/**
 * Propose Plan through the server's planning session, in the tool the person chose, else Auto: the
 * project's default tool or the first installed one (no server default). `installed` says whether this
 * machine has any AI tool, so a refusal names what to do.
 */
export function createServerPlanner(options: {
  choice: (project: Project) => AiToolChoice | undefined;
  installed?: (project: Project) => boolean;
  client: (project: Project) => Api;
  personId: () => string;
}): OrchestrationPlanner {
  const missing = (project: Project) => (options.installed?.(project) === false ? NO_TOOL : CHOOSE_TOOL);
  return {
    availability: (project) =>
      options.choice(project) ? { available: true } : { available: false, reason: missing(project) },
    choice: (project) => options.choice(project),
    async propose(project) {
      const choice = options.choice(project);
      if (!choice) throw new Error(`${missing(project)}.`);
      try {
        const proposal = await options
          .client(project)
          .proposeOrchestrationPlan(
            project.id,
            { ...choice, operation_id: `propose-${crypto.randomUUID().replaceAll('-', '')}` },
            options.personId(),
          );
        if (proposal.outcome === 'existing') return 'A planning session is already working on the plan.';
        // HS2-FDS95D: the planner that finished its turn was given the new proposal.
        if (proposal.outcome === 'replanned') return 'The planning session is planning again. It is in the drawer.';
        return 'Planning started. The planning session is in the drawer.';
      } catch (reason) {
        planningRefusal(reason);
      }
    },
    async update(project) {
      const choice = options.choice(project);
      if (!choice) throw new Error(`${missing(project)}.`);
      try {
        const update = await options
          .client(project)
          .updateOrchestrationPlan(
            project.id,
            { ...choice, operation_id: `update-${crypto.randomUUID().replaceAll('-', '')}` },
            options.personId(),
          );
        if (update.outcome === 'existing') return 'The plan is already being updated.';
        return 'Updating the plan. Work continues on the current plan until the new one is ready.';
      } catch (reason) {
        planningRefusal(reason);
      }
    },
    async cancelUpdate(project) {
      try {
        await options.client(project).cancelOrchestrationPlanUpdate(project.id, options.personId());
        return 'Plan update cancelled. The current plan is unchanged.';
      } catch (reason) {
        planningRefusal(reason);
      }
    },
  };
}

export interface OrchestrationTimelineClient {
  orchestrationTimeline: (checkout: string, after?: string, signal?: AbortSignal) => Promise<OrchestrationTimeline>;
}

/** One cursor long poll of a project's timeline. Failures back off; stopping aborts the open read. */
export function startOrchestrationTimelineStream(options: {
  client: OrchestrationTimelineClient;
  checkout: string;
  onTimeline: (timeline: OrchestrationTimeline) => void;
  onError: (error: unknown) => void;
  delay?: (ms: number, signal: AbortSignal) => Promise<void>;
}): () => void {
  return startVersionedLongPoll({
    read: (after, signal) => options.client.orchestrationTimeline(options.checkout, after, signal),
    onValue: options.onTimeline,
    onError: options.onError,
    delay: options.delay,
  });
}

export interface OrchestrationViewDependencies {
  readonly project: () => Project | undefined;
  readonly viewMode: Signal<WorkspaceViewMode>;
  /** The ticket view the workspace returns to when this project has no Orchestration view. */
  readonly fallbackViewMode: () => WorkspaceViewMode;
  /**
   * HS2-BRRSYM: the view orchestration projects last showed (Orchestration by default). Switching to
   * an orchestration project restores it; choosing a view in one remembers it.
   */
  readonly orchestrationViewMode: Signal<OrchestrationProjectViewMode>;
  readonly mobile: () => boolean;
  readonly confirm: (options: ConfirmationOptions) => Promise<boolean>;
  readonly showToast: (message: string) => void;
  readonly openTicket: (match: TicketLinkMatch) => Promise<void>;
  /** HS2-DBQSWB: open this exact orchestrator permission request's card in Notifications. */
  readonly showPermissionRequest: (requestId: string) => Promise<void>;
  /** Open Orchestration settings (HS2-W27N1D), or Settings until that category exists. */
  readonly openOrchestrationSettings: () => void;
  readonly persistWorkspacePreferences: () => void;
  readonly planner: OrchestrationPlanner;
  /** This browser's person, named in run controls and policy changes (they are a person's acts). */
  readonly personId: () => string;
  readonly createClient?: (project: Project) => Api;
  /** HS2-FA8G9J: show an open project's terminal in its drawer; false when no tab of this window holds it. */
  readonly showTerminal?: (projectId: string, terminalId: string) => boolean;
  /**
   * HS2-W27N1D seam: whether a project's Project Type is Orchestration, when the settings surface
   * knows (`orchestrationProject(id)`, refreshed on `orchestration_changed`). `undefined` falls back
   * to this view's own read of the target declaration that Project Type creates.
   */
  readonly orchestrationProject?: (projectId: string) => boolean | undefined;
  /**
   * HS2-9XJEZC: the orchestrator permission requests (by request id) this browser shows as pending, read
   * reactively. A Run Validation request whose permission request was pending and no longer is has been
   * answered, so it is sent again under its operation id.
   */
  readonly pendingOrchestratorRequests?: () => readonly PendingOrchestratorAsk[];
  /** HS2-JGFPG4 seam: the bounded backoff's wait between retries of a transient failure. */
  readonly delay?: BackoffDelay;
}

/** HS2-A4RVYA: how long typing in a template input pauses before the summary is read again. */
export const TEMPLATE_PREVIEW_PAUSE_MS = 400;

const message = (reason: unknown) => (reason instanceof Error ? reason.message : String(reason));

/** The selected run's records as the inspector shows them; an unread record leaves its part absent. */
/** HS2-GD3NNK: a review gate's state in the review's own words; other gates keep theirs. */
function gateState(kind: string | undefined, state: string, reason?: string): string {
  if (kind === 'review') return REVIEW_GATE_STATES[state] ?? state;
  // HS2-507DGZ: an unknown CI gate says why its watch is not running, and what to do.
  const why = state === 'unknown' && reason !== undefined ? CI_WATCH_REASONS[reason] : undefined;
  return why ? `unknown: ${why}` : state;
}

export function parseOrchestrationDetails(
  gates: Record<string, unknown> | undefined,
  events: Record<string, unknown> | undefined,
  validations: ReadonlyArray<Record<string, unknown>> | undefined,
  targetId: string,
  run?: Record<string, unknown>,
  readiness?: Record<string, unknown>,
): OrchestrationInspectorDetails {
  const text = (value: unknown) => (typeof value === 'string' ? value : undefined);
  const gateList = Array.isArray(gates?.gates) ? (gates.gates as Array<Record<string, unknown>>) : [];
  const eventList = Array.isArray(events?.entries) ? (events.entries as Array<Record<string, unknown>>) : [];
  // HS2-7NJN7S: a gate event names its gate, not its kind; superseded gates still name theirs.
  const gateKinds = new Map<unknown, string | undefined>(gateList.map((gate) => [gate.gate_id, text(gate.kind)]));
  return {
    loading: false,
    runRead: run !== undefined,
    reconciliation: parseRunReconciliation(run),
    readinessRead: readiness !== undefined,
    builtAhead: parseBuiltAhead(readiness),
    ci: text(gates?.ci),
    gates: gateList
      .filter((gate) => gate.superseded_by === undefined)
      .map((gate) => ({
        kind: text(gate.kind) ?? 'gate',
        state: gateState(text(gate.kind), text(gate.state) ?? 'unknown', text(gate.reason)),
        provider: text(gate.provider),
        commit: text(gate.commit),
        gate_id: text(gate.gate_id),
        // HS2-A7YCB2: an unknown CI gate whose watch a link starts again offers Link Again.
        relink: relinkRequest(gate),
      })),
    // HS2-9XJEZC: a step's name and target are in its `definition`.
    validations: (validations ?? []).flatMap((evidence) => parseValidationStep(evidence, targetId) ?? []),
    events: eventList
      .map((entry) => (entry.record ?? entry) as Record<string, unknown>)
      .map((record) => ({
        at_ms: typeof record.at_ms === 'number' ? record.at_ms : 0,
        kind: text(record.kind) ?? 'event',
        state: text(record.state),
        reason: text(record.reason),
        source: text(record.source),
        gate_state: text(record.gate_state),
        gate_kind: text(record.gate_id) ? gateKinds.get(record.gate_id) : undefined,
      }))
      .slice(-8)
      .reverse(),
  };
}

/**
 * One scheduled frame serves every placement request made before it runs (HS2-PJ3SE6). A request
 * replaces the pending frame, but a forced request (the board's width changed) is kept until a
 * frame acts on it: `place` returns false when it could not act yet, and the next frame is then
 * still forced. Before this, a timeline update right after a resize replaced the resize's forced
 * frame with an ordinary one that saw the layout already placed and left the old scroll position.
 */
export function coalescedPlacement(
  schedule: (callback: () => void) => number,
  cancel: (handle: number) => void,
  place: (force: boolean) => boolean,
): { request: (force?: boolean) => void; cancel: () => void } {
  let handle = 0,
    forced = false;
  return {
    request: (force = false) => {
      forced ||= force;
      cancel(handle);
      handle = schedule(() => {
        if (place(forced)) forced = false;
      });
    },
    cancel: () => {
      cancel(handle);
    },
  };
}

export function createOrchestrationViewController(dependencies: OrchestrationViewDependencies) {
  const planner = dependencies.planner;
  const clientFor =
    dependencies.createClient ?? ((project: Project) => new Api(project.apiPath, '', { trackBusy: false }));
  /** Per project: whether it declares targets. Absent until read; false when the read failed too. */
  const declared = signal<Partial<Record<string, boolean>>>({});
  const timeline = signal<OrchestrationTimeline | undefined>(undefined);
  /** HS2-HDHT5E: remote target tickets' labels, read while the view is open and kept in memory only. */
  const remoteLabels = signal<Readonly<Record<string, RemoteTicketLabel>>>({});
  const labelled = computed(() => withRemoteLabels(timeline.value, remoteLabels.value));
  let labelReader: ReturnType<typeof createRemoteLabelReader> | undefined;
  const loading = signal(false);
  const error = signal<string | undefined>(undefined);
  const selectedStep = signal<string | undefined>(undefined);
  const details = signal<OrchestrationInspectorDetails | undefined>(undefined);
  const busy = signal(false);
  /** HS2-F1BKQ0: the plan's state while the timeline read fails because the plan cannot be read. */
  const planHealth = signal<PlanHealth | undefined>(undefined);
  /** Bumped to read the timeline again at once, after a person resolved what stopped it. */
  const restarts = signal(0);
  /** Step ids to slugs from the last timeline read, kept while the plan cannot be read. */
  let lastSlugs = new Map<string, string>();
  /** HS2-2XJSJK: with no card selected, what the overview read (HS2-77SRHC: the newest decisions). */
  const overview = signal<
    | {
        decisions?: SchedulerDecisionPage;
        contention?: ResourceContentionView[];
        roles?: RoleStandingView[];
        summary?: ProgressSummaryView;
        /** HS2-70MHXJ: the summary read failed and none was read before. */
        summaryUnreadable?: boolean;
        error?: string;
      }
    | undefined
  >(undefined);
  /** HS2-77SRHC: the open Show All Decisions… dialog and the cursor of the page before what it shows. */
  const decisionsDialog = signal<OrchestrationDecisionsDialogState | undefined>(undefined);
  let decisionsBefore = 0;
  /** HS2-A4RVYA: the open Use Template… dialog. */
  const templateDialog = signal<OrchestrationTemplateDialogState | undefined>(undefined);
  let templateSession = 0,
    templatePreviews = 0,
    /** The inputs and choices of the last preview asked for. */
    lastPreview = '',
    /** A pause in typing reads the summary again (keystroke coalescing, no polling). */
    previewTimer: ReturnType<typeof setTimeout> | undefined,
    /** The operation id of the apply on screen, kept so Apply Again repeats the same apply. */
    applyOperation: string | undefined;
  /** HS2-9XJEZC: Run Validation requests waiting on a person, by `<step id>:<validation step id>`. */
  const pendingValidations = signal<Readonly<Record<string, PendingValidation>>>({});
  /**
   * HS2-HCW7MD: where planning stands (`idle`, `planning`, `ended`, or HS2-5RYXNN `waiting`), read when the
   * timeline changes or a session an orchestrator started here changes.
   */
  /** HS2-N5BDE4: the goal ticket holding the planner's unanswered question. */
  const question = signal<PlanningQuestion | undefined>(undefined);
  /** HS2-YSRT6Y: no explicit goal is open, so planning works through all open tickets. */
  const defaultGoal = signal(false);
  const planning = signal<string | undefined>(undefined);
  /** HS2-FA8G9J: the planning session's terminal, from `plan show`. */
  const plannerTerminal = signal<PlannerTerminal | undefined>(undefined);
  const disposers: Array<() => void> = [];
  let streamKey = '',
    streamStop: (() => void) | undefined,
    generation = 0;
  /** HS2-JGFPG4: aborts the backoff waits of validations followed under the current generation. */
  let follows = new AbortController();
  const probed = new Set<string>();

  const isOrchestrationProject = (projectId: string | undefined) =>
    projectId !== undefined && (dependencies.orchestrationProject?.(projectId) ?? declared.value[projectId] === true);

  const learn = (projectId: string, view: OrchestrationTimeline) => {
    if (declared.value[projectId] !== view.declared) declared.value = { ...declared.value, [projectId]: view.declared };
  };

  // One read per project tells the view switcher whether to offer the tab.
  disposers.push(
    effect(() => {
      const current = dependencies.project();
      if (!current || probed.has(current.id)) return;
      probed.add(current.id);
      clientFor(current)
        .orchestrationTimeline(current.id)
        .then((view) => {
          learn(current.id, view);
        })
        .catch(() => {
          declared.value = { ...declared.value, [current.id]: false };
        });
    }),
  );

  // The view falls back to the ticket view in a project that is not an orchestration project.
  disposers.push(
    effect(() => {
      const current = dependencies.project();
      if (dependencies.viewMode.value !== 'orchestration' || !current) return;
      // Only a project known not to be one falls back; an unread one keeps the view.
      const known = dependencies.orchestrationProject?.(current.id) ?? declared.value[current.id];
      if (known === false) {
        dependencies.viewMode.value = dependencies.fallbackViewMode();
        dependencies.persistWorkspacePreferences();
      }
    }),
  );

  // HS2-BRRSYM: arriving at a project shows the view its type last showed: an orchestration project
  // the one orchestration projects remember, a software project its ticket view. The project's type
  // may be read after the switch, so the arrival waits for it, unless the person chose a view meanwhile.
  let arrivedAt: string | undefined, arrival: { projectId: string; view: WorkspaceViewMode } | undefined;
  disposers.push(
    effect(() => {
      const current = dependencies.project(),
        mode = dependencies.viewMode.value;
      if (current?.id !== arrivedAt) {
        arrivedAt = current?.id;
        arrival = current ? { projectId: current.id, view: mode } : undefined;
      }
      if (!current || arrival?.projectId !== current.id) return;
      const known = dependencies.orchestrationProject?.(current.id) ?? declared.value[current.id];
      if (known === undefined) return;
      const before = arrival.view;
      arrival = undefined;
      if (mode !== before) return;
      const remembered = known ? dependencies.orchestrationViewMode.peek() : dependencies.fallbackViewMode();
      if (!isOrchestrationProjectViewMode(remembered)) return;
      const next = projectArrivalView(mode, remembered);
      if (next === undefined) return;
      dependencies.viewMode.value = next;
      dependencies.persistWorkspacePreferences();
    }),
  );

  // HS2-BRRSYM: a view chosen in an orchestration project is the one orchestration projects remember.
  // Only a change of view counts, not a project switch that leaves the previous project's view showing.
  let seenMode: WorkspaceViewMode | undefined;
  disposers.push(
    effect(() => {
      const mode = dependencies.viewMode.value,
        current = dependencies.project();
      if (mode === seenMode) return;
      seenMode = mode;
      if (!current || !isOrchestrationProject(current.id)) return;
      const remembered = dependencies.orchestrationViewMode.peek(),
        next = rememberOrchestrationProjectView(mode, remembered);
      if (next === remembered) return;
      dependencies.orchestrationViewMode.value = next;
      dependencies.persistWorkspacePreferences();
    }),
  );

  // The long poll runs while the view is open.
  disposers.push(
    effect(() => {
      const current = dependencies.project(),
        key =
          current && dependencies.viewMode.value === 'orchestration'
            ? `${current.id}:${current.apiPath}:${restarts.value}`
            : '';
      if (key === streamKey) return;
      streamStop?.();
      streamStop = undefined;
      labelReader?.reset();
      labelReader = undefined;
      streamKey = key;
      const run = ++generation;
      follows.abort();
      follows = new AbortController();
      batch(() => {
        timeline.value = undefined;
        remoteLabels.value = {};
        error.value = undefined;
        selectedStep.value = undefined;
        details.value = undefined;
        overview.value = undefined;
        decisionsDialog.value = undefined;
        planHealth.value = undefined;
        loading.value = Boolean(key);
        planning.value = undefined;
        plannerTerminal.value = undefined;
      });
      if (!key || !current) return;
      const reader = createRemoteLabelReader({
        read: (ref, signal) =>
          clientFor(current).orchestrationTicketRead(current.id, ref, dependencies.personId(), signal),
        onLabels: (labels) => {
          if (run === generation) remoteLabels.value = labels;
        },
      });
      labelReader = reader;
      streamStop = startOrchestrationTimelineStream({
        client: clientFor(current),
        checkout: current.id,
        onTimeline: (view) => {
          if (run !== generation) return;
          batch(() => {
            planHealth.value = undefined;
            lastSlugs = new Map(
              view.waves.flatMap((wave) =>
                wave.items.flatMap((item) => (item.slug ? [[item.step_id, item.slug]] : [])),
              ),
            );
            learn(current.id, view);
            timeline.value = view;
            loading.value = false;
            error.value = undefined;
            if (selectedStep.value && !findTimelineItem(view, selectedStep.value)) selectedStep.value = undefined;
          });
          reader.fill(view);
          placeTimeline();
          void refreshDetails();
          void refreshOverview();
          void refreshPlanning(current, run);
        },
        onError: (reason) => {
          if (run !== generation) return;
          batch(() => {
            loading.value = false;
            error.value = message(reason);
          });
          // HS2-F1BKQ0: a plan that cannot be read says why, with the steps involved.
          const code = (reason as { code?: string } | undefined)?.code;
          if (code && PLAN_FAILURE_CODES.has(code))
            void clientFor(current)
              .orchestrationPlanHealth(current.id)
              .then((value) => {
                if (run === generation) planHealth.value = parsePlanHealth(value);
              })
              .catch(() => undefined);
          else planHealth.value = undefined;
        },
      });
    }),
  );

  /**
   * The timeline reads newest on the left and Done on the right, so it opens scrolled to its right end
   * where current work is; a phone (paged) opens on Working Now. Kerf ColumnTrack has no start-at-end
   * option (KF-QHJKA4), so this sets the scroll position once per board layout,
   * and again when the board's width changes (a window resize, the inspector opening).
   */
  const placed = new WeakMap<Element, string>();
  let observed: { track: HTMLElement; observer: ResizeObserver; width: number } | undefined;
  function observe(track: HTMLElement) {
    if (observed?.track === track || typeof ResizeObserver !== 'function') return;
    observed?.observer.disconnect();
    const watch = {
      track,
      width: track.clientWidth,
      observer: new ResizeObserver(() => {
        if (!track.isConnected || track.clientWidth === watch.width) return;
        watch.width = track.clientWidth;
        if (dependencies.viewMode.value === 'orchestration') placeTimeline(true);
      }),
    };
    watch.observer.observe(track);
    observed = watch;
  }
  let tries = 0;
  const placement = coalescedPlacement(
    (callback) => requestAnimationFrame(callback),
    (handle) => {
      cancelAnimationFrame(handle);
    },
    (force) => {
      const view = timeline.value;
      const board = document.querySelector<HTMLElement>(
        '[data-orchestration-view="true"] [data-component="ticket-board"]',
      );
      if (!view || !board) return false;
      // A layout switch remounts the board after this frame; wait for the new one.
      if (board.dataset.layout !== (dependencies.mobile() ? 'paged' : 'grid')) {
        if (tries > 0) {
          tries -= 1;
          placement.request();
        }
        return false;
      }
      const columns = [...board.querySelectorAll<HTMLElement>('[data-component="ticket-board-column"]')];
      if (columns.length === 0) return false;
      let track: HTMLElement | null = columns[0].parentElement;
      while (track && track !== board && !/auto|scroll/.test(getComputedStyle(track).overflowX))
        track = track.parentElement;
      if (!track || track === board) return false;
      observe(track);
      const layout = `${board.dataset.layout}:${columns.map((column) => column.dataset.columnId).join(',')}`;
      if (!force && placed.get(track) === layout) return true;
      placed.set(track, layout);
      const start = columns.find((column) => column.dataset.columnId === startColumnId(view));
      if (board.dataset.layout === 'paged' && start)
        track.scrollLeft += start.getBoundingClientRect().left - track.getBoundingClientRect().left;
      else {
        track.scrollLeft = track.scrollWidth;
        const clipped = start ? start.getBoundingClientRect().left - track.getBoundingClientRect().left : 0;
        if (clipped < 0) track.scrollLeft += clipped;
      }
      return true;
    },
  );
  function placeTimeline(again = false) {
    if (typeof requestAnimationFrame !== 'function') return;
    tries = 8;
    placement.request(again);
  }
  // A layout change (wide to phone and back) places the board again.
  disposers.push(
    effect(() => {
      dependencies.mobile();
      if (dependencies.viewMode.value === 'orchestration') placeTimeline();
    }),
  );

  async function refreshPlanning(current: Project, run: number) {
    try {
      const view = await clientFor(current).orchestrationPlanning(current.id);
      if (run === generation)
        batch(() => {
          planning.value = planningPhase(view);
          plannerTerminal.value = plannerTerminalOf(view);
          question.value = planningQuestion(view);
          defaultGoal.value = planningDefaultGoal(view);
        });
    } catch {
      if (run === generation) {
        planning.value = undefined;
        plannerTerminal.value = undefined;
      }
    }
  }

  const selected = () => findTimelineItem(labelled.value, selectedStep.value)?.item;

  let detailsRequest = 0;
  async function refreshDetails() {
    const current = dependencies.project(),
      item = selected(),
      run = item?.run;
    const request = ++detailsRequest;
    // HS2-77SRHC: a ready card reads why it is not running from the scheduler itself.
    if (current && item?.state === 'ready' && !run) {
      const explained = await clientFor(current)
        .orchestrationSchedulerExplain(current.id, item.step_id, dependencies.personId())
        .catch(() => undefined);
      if (request === detailsRequest) details.value = { loading: false, explanation: parseExplanation(explained) };
      return;
    }
    if (!current || !item || !run) {
      details.value = undefined;
      return;
    }
    details.value = { ...(details.value ?? {}), loading: true };
    const client = clientFor(current);
    const [gates, events, validations, record, readiness, explained] = await Promise.allSettled([
      client.orchestrationRunGates(current.id, run.run_id),
      client.orchestrationRunEvents(current.id, run.run_id),
      client.orchestrationValidations(current.id, item.goal),
      client.orchestrationRun(current.id, run.run_id),
      client.orchestrationReadiness(current.id, item.step_id),
      // HS2-65AD6J: whether the project's integrator runs for the work, read with the run's records.
      client.orchestrationSchedulerExplain(current.id, item.step_id, dependencies.personId()),
    ]);
    if (request !== detailsRequest) return;
    const value = <T>(result: PromiseSettledResult<T>) => (result.status === 'fulfilled' ? result.value : undefined);
    const pending = Object.values(pendingValidations.value).filter((entry) =>
      entry.operation_id.startsWith(`validate-${item.step_id}-`),
    );
    const failed = [gates, events, record].some((result) => result.status === 'rejected');
    details.value = {
      ...parseOrchestrationDetails(
        value(gates),
        value(events),
        value(validations),
        item.target.target_id,
        value(record),
        value(readiness),
      ),
      error: failed ? 'Some of this run’s records could not be read.' : undefined,
      integrator: parseIntegratorStanding(value(explained)),
      held: heldExplanation(value(explained)),
      pendingValidations: pending,
    };
  }

  /**
   * HS2-9XJEZC: while a card with a run is selected, one cursor long poll of that run's events reads the
   * card's details again on each new event: a validation result, a gate, a reconciliation or provisional
   * change that leaves the timeline as it was. It ends when the selection, project or view changes.
   */
  let eventsKey = '',
    eventsStop: (() => void) | undefined;
  disposers.push(
    effect(() => {
      const current = dependencies.project(),
        open = dependencies.viewMode.value === 'orchestration',
        runId = findTimelineItem(timeline.value, selectedStep.value)?.item.run?.run_id;
      const key = current && open && runId ? `${current.id}:${current.apiPath}:${runId}` : '';
      if (key === eventsKey) return;
      eventsStop?.();
      eventsStop = undefined;
      eventsKey = key;
      if (!key || !current || !runId) return;
      let first = true;
      eventsStop = startVersionedLongPoll({
        read: async (after, signal) => {
          const page = await clientFor(current).orchestrationRunEventsAfter(current.id, runId, after, signal);
          return { version: typeof page.cursor === 'number' ? String(page.cursor) : (after ?? '0') };
        },
        onValue: () => {
          // The first answer is the cursor the details were read at; later ones are new events.
          if (first) first = false;
          else void refreshDetails();
        },
        onError: () => undefined,
      });
    }),
  );

  // HS2-9XJEZC: a request seen pending that is no longer pending was answered; send its operation again.
  const seenPending = new Set<string>();
  disposers.push(
    effect(() => {
      const asks = dependencies.pendingOrchestratorRequests?.();
      const current = dependencies.project();
      if (!asks || !current) return;
      const shown = new Set(asks.map((ask) => ask.request_id));
      // HS2-NY0MRJ: requests this browser no longer remembers (after a reload) are recovered from the
      // pending permission list, so their rows wait for permission and the answer still runs them once.
      const known = pendingValidations.peek();
      const recovered = recoveredValidations(asks, current.id).filter((entry) => !(entry.operation_id in known));
      if (recovered.length > 0) {
        pendingValidations.value = {
          ...known,
          ...Object.fromEntries(recovered.map((entry) => [entry.operation_id, entry])),
        };
        void refreshDetails();
      }
      const waiting = Object.values(pendingValidations.peek());
      if (waiting.length === 0) return;
      const answered = waiting.filter((entry) => {
        if (shown.has(entry.request_id)) {
          seenPending.add(entry.request_id);
          return false;
        }
        return seenPending.has(entry.request_id);
      });
      if (answered.length === 0) return;
      for (const entry of answered) seenPending.delete(entry.request_id);
      void sendValidations(answered, false);
    }),
  );

  /**
   * HS2-9XJEZC: ask the orchestrator to run each step, through the same gate as its own loop: a
   * remembered rule lets a profile step run, and anything else (a command step always) files the
   * ordinary permission request and waits for a person. A refusal with a request id is kept and sent
   * again under the same operation id once that request is answered; a denial is final. A started step
   * is followed under its operation id (each call waits up to 8 seconds on the server) until its result
   * is final, which is how the orchestrator collects a result (§26.12). Each call names the person who
   * pressed Run Validation as `requested_by`, recorded in the audit only (HS2-ZYM6ZT).
   */
  async function sendValidations(requests: readonly PendingValidation[], announce: boolean) {
    const current = dependencies.project();
    if (!current) return;
    const client = clientFor(current),
      run = generation;
    // HS2-NY0MRJ: each request is for the card its operation names, which need not be the selected one.
    const itemOf = (entry: PendingValidation) => {
      const planStep = parseValidationOperation(entry.operation_id)?.planStep;
      return (planStep ? findTimelineItem(timeline.value, planStep)?.item : undefined) ?? selected();
    };
    const call = (entry: PendingValidation, waitMs: number) => {
      const item = itemOf(entry);
      if (!item) return Promise.reject(new Error('The card of this validation is no longer planned.'));
      return client.orchestrationValidationRun(current.id, {
        ticket: item.goal,
        step_id: entry.step_id,
        operation_id: entry.operation_id,
        run_id: item.run?.run_id,
        wait_ms: waitMs,
        // HS2-ZYM6ZT: the audit names this person beside the orchestrator that acts.
        requested_by: dependencies.personId(),
      });
    };
    const unsettled = (answer: unknown) => validationRunUnsettled(parseValidationRunAnswer(answer));
    const signal = follows.signal;
    const follow = async (entry: PendingValidation) => {
      // At most an hour of eight-second waits: the longest a step may run (§26.12), and a bound.
      for (let wait = 0; wait < 450 && run === generation; wait += 1) {
        try {
          // HS2-JGFPG4: a dropped request is asked again on the bounded backoff, not given up on.
          const answer = await retryTransient(() => call(entry, 8_000), {
            signal,
            delay: dependencies.delay,
            wanted: () => run === generation,
            // A card no longer planned stays unplanned; only the request's own failures are retried.
            transient: (reason) => itemOf(entry) !== undefined && isTransientRequestFailure(reason),
          });
          void refreshDetails();
          if (!unsettled(answer)) return;
        } catch (reason) {
          if (signal.aborted || run !== generation) return;
          // HS2-1HGTD7: a request with no answer reads the same in every browser.
          const why = reason instanceof ApiHttpError ? describeRefusal(reason) : describeRequestFailure(reason);
          dependencies.showToast(`Validation step ${entry.step_id} is no longer followed: ${why}`);
          void refreshDetails();
          return;
        }
      }
    };
    const next = new Map(Object.entries(pendingValidations.value));
    let asked = 0,
      denied = 0,
      failed = '';
    for (const entry of requests) {
      const key = entry.operation_id;
      try {
        const answer = await call(entry, 0);
        next.delete(key);
        if (unsettled(answer)) void follow(entry);
      } catch (reason) {
        const refusal = reason instanceof ApiHttpError ? reason : undefined;
        if (refusal?.code === 'permission_required' && refusal.requestId) {
          if (!next.has(key)) asked += 1;
          next.set(key, { ...entry, request_id: refusal.requestId });
        } else {
          next.delete(key);
          if (refusal?.code === 'permission_denied') denied += 1;
          else failed ||= describeRefusal(reason);
        }
      }
    }
    const settled = Object.fromEntries(next);
    const changed = JSON.stringify(settled) !== JSON.stringify(pendingValidations.value);
    pendingValidations.value = settled;
    if (announce && asked > 0)
      dependencies.showToast(
        asked === 1 ? 'A validation step waits for permission.' : `${asked} validation steps wait for permission.`,
      );
    if (denied > 0) dependencies.showToast('A person denied running a validation step.');
    if (failed) dependencies.showToast(`Validation could not start: ${failed}`);
    if (changed || announce) void refreshDetails();
  }

  async function runValidation() {
    const item = selected(),
      steps = details.value?.validations ?? [];
    if (!item || busy.value) return;
    const prefix = `validate-${item.step_id}-`;
    const waiting = new Set(
      Object.values(pendingValidations.value)
        .filter((entry) => entry.operation_id.startsWith(prefix))
        .map((entry) => entry.step_id),
    );
    const requests = stepsToRun(steps)
      .filter((step) => !waiting.has(step.step_id))
      .map((step) => ({
        step_id: step.step_id,
        operation_id: validationOperation(
          item.step_id,
          step.step_id,
          crypto.randomUUID().replaceAll('-', '').slice(0, 12),
        ),
        request_id: '',
      }));
    if (requests.length === 0) return;
    busy.value = true;
    try {
      await sendValidations(requests, true);
    } finally {
      busy.value = false;
    }
  }

  /** HS2-2XJSJK: the overview's reads, while the view shows it (no card selected). */
  let overviewRequest = 0;
  async function refreshOverview() {
    const current = dependencies.project();
    const request = ++overviewRequest;
    if (!current || selectedStep.value || dependencies.viewMode.value !== 'orchestration') return;
    const client = clientFor(current);
    // HS2-5GRXFN: the scheduler's own dry-run plan says which resources hold work back; it logs nothing.
    const [page, plan, summary] = await Promise.allSettled([
      client.orchestrationDecisions(current.id, { limit: 30 }),
      client.orchestrationSchedulerPlan(current.id, dependencies.personId()),
      // HS2-XJEH08: progress and cost from this machine's records.
      client.orchestrationSummary(current.id),
    ]);
    if (request !== overviewRequest) return;
    overview.value = {
      decisions: page.status === 'fulfilled' ? parseDecisionPage(page.value) : overview.value?.decisions,
      contention: plan.status === 'fulfilled' ? parseContention(plan.value) : overview.value?.contention,
      // HS2-65AD6J: each project's integrator and the role sessions this pass does not start.
      roles: plan.status === 'fulfilled' ? parseRoleStandings(plan.value) : overview.value?.roles,
      summary: summary.status === 'fulfilled' ? parseProgressSummary(summary.value) : overview.value?.summary,
      summaryUnreadable: summary.status === 'rejected' && overview.value?.summary === undefined,
      error:
        page.status === 'rejected' || plan.status === 'rejected' || summary.status === 'rejected'
          ? 'Some of the scheduler’s records could not be read.'
          : undefined,
    };
  }

  const openDecisions = () => decisionsDialog.value;
  /** HS2-77SRHC: Show All Decisions… opens the log, newest first; Load Earlier reads the page before it. */
  async function readDecisions(earlier: boolean) {
    const current = dependencies.project(),
      open = decisionsDialog.value;
    if (!current || !open) return;
    decisionsDialog.value = { ...open, loading: true, error: undefined };
    try {
      const page = parseDecisionPage(
        await clientFor(current).orchestrationDecisions(current.id, {
          limit: 50,
          before: earlier ? decisionsBefore : undefined,
        }),
      );
      // The person may have closed the dialog while the page was read.
      const shown = openDecisions();
      if (!shown) return;
      decisionsBefore = page.cursor;
      decisionsDialog.value = {
        entries: earlier ? [...shown.entries, ...page.entries] : page.entries,
        more: page.more,
        loading: false,
      };
    } catch (reason) {
      const shown = openDecisions();
      if (shown) decisionsDialog.value = { ...shown, loading: false, error: message(reason) };
    }
  }

  function showDecisions() {
    decisionsDialog.value = { entries: [], more: false, loading: true };
    void readDecisions(false);
  }

  function select(stepId: string) {
    if (selectedStep.value === stepId) return;
    batch(() => {
      selectedStep.value = stepId;
      details.value = undefined;
    });
    void refreshDetails();
  }

  function toggleGroup(group: string) {
    const current = dependencies.project();
    if (current) toggleTicketGroupCollapsed(current.id, ORCHESTRATION_GROUP_SCOPE, group);
  }

  function collapsedGroups(): string[] {
    const current = dependencies.project(),
      view = timeline.value;
    if (!current || !view) return [];
    return collapsedTicketGroupKeys(
      current.id,
      ORCHESTRATION_GROUP_SCOPE,
      view.targets.map((target) => target.target_id),
    );
  }

  /**
   * Start or pause by turning the policy's automatic placement on or off (HS2-ECWG50). HS2-D9KP86: with no
   * saved policy the press saves the default one (every member left to the server's default, which is what
   * Settings › Orchestration › Policy shows) with placement set, in the same press. HS2-TZJYKH: the server's
   * runtime makes the edit at the stored revision, as `policy start|pause` does.
   */
  async function setAutomatic(automatic: boolean) {
    const current = dependencies.project();
    if (!current || busy.value) return;
    busy.value = true;
    try {
      const set = await clientFor(current).setOrchestrationAutomatic(current.id, automatic);
      dependencies.showToast(
        automatic
          ? set.saved_default
            ? 'Saved the default policy and started orchestration.'
            : 'Orchestration started.'
          : 'Orchestration paused. Running work continues.',
      );
    } catch (reason) {
      dependencies.showToast(`${automatic ? 'Start' : 'Pause'} failed: ${describeRefusal(reason)}`);
    } finally {
      busy.value = false;
    }
  }

  async function runControl(control: string, state?: string) {
    const current = dependencies.project(),
      item = selected(),
      run = item?.run;
    if (!current || !item || !run || busy.value) return;
    const name = item.slug ?? item.project_label;
    const ask: Partial<Record<string, ConfirmationOptions>> = {
      handoff: {
        title: `Hand ${name} to a person?`,
        message: 'The run lets go of its claim and keeps following the ticket. Nobody is asked to take it over.',
        confirmLabel: 'Hand Off',
      },
      cancel: {
        title: `Cancel the run of ${name}?`,
        message: 'Its claim is released or its Up Next mark withdrawn, and the scheduler does not start it again.',
        confirmLabel: 'Cancel Run',
        cancelLabel: 'Keep Run',
        tone: 'danger',
      },
      correct: {
        title: `Record the run of ${name} as ${(state ?? '').replaceAll('_', ' ')}?`,
        message: 'This rewrites this machine’s record by hand. Nothing changes in the project itself.',
        confirmLabel: 'Correct Record',
        tone: 'danger',
      },
    };
    const question = ask[control];
    if (question && !(await dependencies.confirm(question))) return;
    busy.value = true;
    const person = dependencies.personId();
    try {
      const client = clientFor(current);
      if (control === 'pause') await client.orchestrationRunControl(current.id, run.run_id, 'pause', person, {});
      else if (control === 'resume') await client.orchestrationRunControl(current.id, run.run_id, 'resume', person);
      else if (control === 'handoff')
        await client.orchestrationRunControl(current.id, run.run_id, 'handoff', person, {});
      else if (control === 'cancel') await client.orchestrationRunControl(current.id, run.run_id, 'cancel', person);
      else if (control === 'correct' && state)
        await client.orchestrationRunControl(current.id, run.run_id, 'correct', person, {
          state,
          reason: 'person_correction',
        });
    } catch (reason) {
      dependencies.showToast(`The run could not be changed: ${describeRefusal(reason)}`);
    } finally {
      busy.value = false;
    }
  }

  /**
   * HS2-A7YCB2: a person links the selected run's unknown CI gate again through the gate link route, with
   * the gate's own commit and scope. The gate event that follows reads the details again.
   */
  async function linkGate(gateId: string) {
    const current = dependencies.project(),
      run = selected()?.run,
      request = details.value?.gates?.find((gate) => gate.gate_id === gateId)?.relink;
    if (!current || !run || !request || busy.value) return;
    busy.value = true;
    try {
      await clientFor(current).orchestrationGateLink(current.id, run.run_id, { ...request }, dependencies.personId());
      await refreshDetails();
    } catch (reason) {
      dependencies.showToast(`The gate could not be linked again: ${describeRefusal(reason)}`);
    } finally {
      busy.value = false;
    }
  }

  /** HS2-0D60T8: a person reopens the selected card's built-ahead work; the record and note follow. */
  async function reopen(reason: string) {
    const current = dependencies.project(),
      item = selected();
    if (!current || !item || busy.value) return;
    const name = item.slug ?? item.project_label;
    const confirmed = await dependencies.confirm({
      title: `Reopen the work on ${name}?`,
      message:
        'It is marked reopened and never confirmed by a later CI result. Its run keeps going; cancel or hand it off with the run’s own controls.',
      confirmLabel: 'Reopen',
      tone: 'danger',
    });
    if (!confirmed) return;
    busy.value = true;
    try {
      await clientFor(current).orchestrationReopen(current.id, item.step_id, reason, dependencies.personId());
      dependencies.showToast(`The work on ${name} was reopened.`);
    } catch (reason) {
      dependencies.showToast(`The work could not be reopened: ${describeRefusal(reason)}`);
    } finally {
      busy.value = false;
    }
    void refreshDetails();
  }

  /**
   * HS2-F1BKQ0: a person keeps the checked-in plan, dropping exactly this machine's conflicting
   * pre-sharing definitions (`readiness migrate` with `discard_local`, a person's act), then the view
   * reads the plan again at once.
   */
  async function keepCheckedInPlan() {
    const current = dependencies.project(),
      health = planHealth.value;
    if (!current || !health?.discard_resolves || busy.value) return;
    const confirmed = await dependencies.confirm({
      title: 'Keep the checked-in plan?',
      message: `This machine’s copies of ${health.local.map(stepName).join(', ')} are dropped for the checked-in steps. This cannot be undone here.`,
      confirmLabel: 'Keep the Checked-In Plan',
      tone: 'danger',
    });
    if (!confirmed) return;
    busy.value = true;
    try {
      await clientFor(current).orchestrationPlanMigrate(current.id, true, dependencies.personId());
      dependencies.showToast('This machine now uses the checked-in plan.');
      restarts.value += 1;
    } catch (reason) {
      dependencies.showToast(`The plan could not be changed: ${describeRefusal(reason)}`);
    } finally {
      busy.value = false;
    }
  }

  /** HS2-A4RVYA: change the open template dialog, if it is still the session `session` opened. */
  function updateTemplates(
    session: number,
    change: (state: OrchestrationTemplateDialogState) => Partial<OrchestrationTemplateDialogState>,
  ) {
    const state = templateDialog.value;
    if (state?.session === session) templateDialog.value = { ...state, ...change(state) };
  }

  /** HS2-A4RVYA: Use Template…: read the templates and the goals that can be saved, then select the first. */
  async function openTemplates(select?: string) {
    const current = dependencies.project();
    if (!current) return;
    const session = ++templateSession;
    applyOperation = undefined;
    lastPreview = '';
    templateDialog.value = {
      loading: true,
      templates: [],
      mode: 'template',
      detailActive: false,
      inputs: {},
      changed: [],
      choices: {},
      previewing: false,
      busy: false,
      projects: (timeline.value?.targets ?? []).map((target) => ({ target_id: target.target_id, label: target.label })),
      save: { goal: '', name: '', description: '', targetsAsInputs: false },
      goals: [],
      session,
    };
    const client = clientFor(current);
    const [listed, planningView] = await Promise.allSettled([
      client.orchestrationTemplates(current.id, 'list', {}, dependencies.personId()),
      client.orchestrationPlanning(current.id),
    ]);
    const goals = planningView.status === 'fulfilled' ? savableGoals(planningView.value) : [];
    if (listed.status === 'rejected') {
      updateTemplates(session, () => ({ loading: false, error: describeRefusal(listed.reason), goals }));
      return;
    }
    const list = parseTemplateList(listed.value);
    const first = list.templates.find(
      (template) => template.state === 'active' && (select === undefined || template.template_id === select),
    );
    updateTemplates(session, () => ({ loading: false, templates: list.templates, goals }));
    if (first) selectTemplate(first.template_id, false);
  }

  function selectTemplate(templateId: string, drill = true) {
    const state = templateDialog.value,
      template = state?.templates.find((candidate) => candidate.template_id === templateId);
    if (!state || !template) return;
    applyOperation = undefined;
    lastPreview = '';
    templateDialog.value = {
      ...state,
      selected: templateId,
      mode: 'template',
      detailActive: drill,
      inputs: initialInputs(template),
      changed: [],
      choices: {},
      preview: undefined,
      result: undefined,
      failure: undefined,
    };
    void previewTemplate();
  }

  /** The pre-run summary of the selected template with the inputs as they are. Writes nothing. */
  async function previewTemplate() {
    const current = dependencies.project(),
      state = templateDialog.value,
      template = state?.templates.find((candidate) => candidate.template_id === state.selected);
    if (!current || !state || !template) return;
    // The same inputs and choices again (a change and a focusout of one edit) read nothing new.
    const asked = JSON.stringify([template.template_id, inputsToSend(template, state.inputs), state.choices]);
    if (asked === lastPreview && (state.previewing || state.preview?.template_id === template.template_id)) return;
    lastPreview = asked;
    const request = ++templatePreviews,
      session = state.session,
      sentChanges = new Set(state.changed);
    updateTemplates(session, () => ({ previewing: true }));
    try {
      const answer = await clientFor(current).orchestrationTemplates(
        current.id,
        'preview',
        {
          template_id: template.template_id,
          inputs: inputsToSend(template, state.inputs),
          choices: state.choices,
        },
        dependencies.personId(),
      );
      if (request !== templatePreviews) return;
      updateTemplates(session, (now) => ({
        previewing: false,
        preview: parseTemplatePreview(answer),
        // Inputs changed while the preview was on its way are judged by the next one.
        changed: now.changed.filter((key) => !sentChanges.has(key) || now.inputs[key] !== state.inputs[key]),
      }));
    } catch (reason) {
      if (request === templatePreviews)
        updateTemplates(session, () => ({ previewing: false, failure: describeRefusal(reason) }));
    }
  }

  /** A typed input changed; `commit` (a change, not each keystroke) also reads the summary again. */
  function editTemplateInput(key: string, value: unknown, commit: boolean) {
    const state = templateDialog.value;
    if (!state || state.mode !== 'template') return;
    // A different value is a different apply; leaving a field unchanged keeps the one on screen.
    if (state.inputs[key] !== value) applyOperation = undefined;
    templateDialog.value = {
      ...state,
      inputs: { ...state.inputs, [key]: value },
      // Only a different value waits for the next summary; committing the same value changes nothing.
      changed: state.changed.includes(key) || state.inputs[key] === value ? state.changed : [...state.changed, key],
      failure: undefined,
    };
    if (previewTimer !== undefined) clearTimeout(previewTimer);
    previewTimer = undefined;
    if (commit) void previewTemplate();
    else
      previewTimer = setTimeout(() => {
        previewTimer = undefined;
        void previewTemplate();
      }, TEMPLATE_PREVIEW_PAUSE_MS);
  }

  /** A step switched between searching first and always creating a new ticket. */
  function chooseTemplateStep(step: string, create: boolean) {
    const state = templateDialog.value;
    if (!state) return;
    applyOperation = undefined;
    const choices: Record<string, StepTicketChoice> = Object.fromEntries(
      Object.entries(state.choices).filter(([key]) => key !== step),
    );
    if (create) choices[step] = 'create';
    templateDialog.value = { ...state, choices, failure: undefined };
    void previewTemplate();
  }

  /**
   * Apply Template: open a new goal ticket and hand it to the planner. The operation id is kept while the same
   * apply is repeated (Apply Again), so a retry never opens a second goal or starts a second planner.
   */
  async function applyTemplate() {
    const current = dependencies.project(),
      state = templateDialog.value,
      template = state?.templates.find((candidate) => candidate.template_id === state.selected);
    if (!current || !state || !template || state.busy) return;
    const session = state.session,
      choice = planner.choice?.(current);
    if (!choice) {
      updateTemplates(session, () => ({ failure: `${planner.availability(current).reason ?? 'Choose an AI tool'}.` }));
      return;
    }
    applyOperation ??= newApplyOperationId();
    updateTemplates(session, () => ({ busy: true, failure: undefined }));
    try {
      const answer = await clientFor(current).orchestrationTemplates(
        current.id,
        'apply',
        {
          template_id: template.template_id,
          inputs: inputsToSend(template, state.inputs),
          choices: state.choices,
          operation_id: applyOperation,
          ...choice,
        },
        dependencies.personId(),
      );
      const result = parseTemplateApply(answer);
      updateTemplates(session, () => ({ busy: false, mode: 'result', detailActive: true, result }));
      void refreshPlanning(current, generation);
    } catch (reason) {
      updateTemplates(session, () => ({ busy: false, failure: describeRefusal(reason) }));
    }
  }

  function openSaveTemplate() {
    const state = templateDialog.value;
    if (!state) return;
    templateDialog.value = {
      ...state,
      mode: 'save',
      detailActive: true,
      failure: undefined,
      save: { ...state.save, goal: state.save.goal || (state.goals[0]?.native_id ?? '') },
    };
  }

  function editSaveTemplate(change: Partial<SaveGoalForm>) {
    const state = templateDialog.value;
    if (state?.mode === 'save')
      templateDialog.value = { ...state, save: { ...state.save, ...change }, failure: undefined };
  }

  function cancelSaveTemplate() {
    const state = templateDialog.value;
    if (state) templateDialog.value = { ...state, mode: 'template', detailActive: false, failure: undefined };
  }

  /** Save a Goal as Template…: the goal's current plan steps become a new template, then it is selected. */
  async function submitSaveTemplate() {
    const current = dependencies.project(),
      state = templateDialog.value;
    if (!current || !state || state.busy || saveProblem(state.save, state.goals)) return;
    const goal = state.goals.find((candidate) => candidate.native_id === state.save.goal);
    if (!goal) return;
    const session = state.session;
    updateTemplates(session, () => ({ busy: true, failure: undefined }));
    try {
      const saved = parsePlanTemplate(
        await clientFor(current).orchestrationTemplates(
          current.id,
          'save_goal',
          {
            goal: { connection_id: goal.connection_id, native_id: goal.native_id },
            name: state.save.name.trim(),
            ...(state.save.description.trim() ? { description: state.save.description.trim() } : {}),
            targets_as_inputs: state.save.targetsAsInputs,
          },
          dependencies.personId(),
        ),
      );
      dependencies.showToast(`Saved the template ${saved?.name ?? state.save.name.trim()}.`);
      await openTemplates(saved?.template_id);
    } catch (reason) {
      updateTemplates(session, () => ({ busy: false, failure: describeRefusal(reason) }));
    }
  }

  /** Open the goal ticket the apply opened, and close the dialog. */
  async function openTemplateGoal() {
    const current = dependencies.project(),
      goal = templateDialog.value?.result?.goal;
    if (!current || !goal) return;
    templateDialog.value = undefined;
    const qualified = `${goal.connection_id}:${goal.native_id}`;
    await dependencies.openTicket({
      projectId: current.id,
      projectName: current.name,
      ticketId: qualified,
      qualifiedId: qualified,
      connectionId: goal.connection_id,
      slug: goal.slug ?? goal.native_id,
      title: '',
    });
  }

  function stepName(stepId: string): string {
    return lastSlugs.get(stepId) ?? `step ${stepId.slice(-6)}`;
  }

  async function propose() {
    const current = dependencies.project();
    if (!current || busy.value || !planner.availability(current).available) return;
    busy.value = true;
    try {
      dependencies.showToast(await planner.propose(current));
      planning.value = 'planning';
    } catch (reason) {
      dependencies.showToast(`Propose Plan failed: ${describeRefusal(reason)}`);
    } finally {
      busy.value = false;
    }
  }

  /** HS2-190RDX: Update Plan, while the current plan keeps running; no pause is needed. */
  async function updatePlan() {
    const current = dependencies.project();
    if (!current || busy.value || !planner.availability(current).available) return;
    busy.value = true;
    try {
      dependencies.showToast(await planner.update(current));
      planning.value = 'updating';
    } catch (reason) {
      dependencies.showToast(`Update Plan failed: ${describeRefusal(reason)}`);
    } finally {
      busy.value = false;
    }
  }

  async function cancelPlanUpdate() {
    const current = dependencies.project();
    if (!current || busy.value) return;
    busy.value = true;
    try {
      dependencies.showToast(await planner.cancelUpdate(current));
      void refreshPlanning(current, generation);
    } catch (reason) {
      dependencies.showToast(`Cancel Plan Update failed: ${describeRefusal(reason)}`);
    } finally {
      busy.value = false;
    }
  }

  /**
   * Orchestrator requests surface where every permission request does (HS2-XHQ1EH): Notifications.
   * Show Request opens the selected step's own request there (HS2-DBQSWB); with no request id it
   * falls back to the selected card's.
   */
  async function showRequest(requestId?: string) {
    const id = requestId || selected()?.permission?.request_id;
    if (id) await dependencies.showPermissionRequest(id);
  }

  /** HS2-N5BDE4: open the goal ticket where the planner asked its question, to answer it there. */
  /** HS2-FA8G9J: open the drawer on the planner's terminal tab, or say this window does not hold it. */
  function showPlanner() {
    const current = dependencies.project(),
      terminal = plannerTerminal.value;
    if (!current || !terminal) return;
    const shown =
      dependencies.showTerminal?.(terminal.checkout_id, terminal.session_id) === true ||
      (terminal.checkout_id !== current.id && dependencies.showTerminal?.(current.id, terminal.session_id) === true);
    if (!shown) dependencies.showToast('The planning session’s terminal is not open in this window.');
  }

  async function answerQuestion() {
    const current = dependencies.project(),
      asked = question.value;
    if (!current || !asked) return;
    const qualified = `${asked.connection_id}:${asked.native_id}`;
    await dependencies.openTicket({
      projectId: current.id,
      projectName: current.name,
      ticketId: qualified,
      qualifiedId: qualified,
      connectionId: asked.connection_id,
      slug: asked.slug,
      title: '',
    });
  }

  async function openGoal() {
    const current = dependencies.project(),
      item = selected();
    if (!current || !item) return;
    const qualified = `${item.goal.connection_id}:${item.goal.native_id}`;
    await dependencies.openTicket({
      projectId: current.id,
      projectName: current.name,
      ticketId: qualified,
      qualifiedId: qualified,
      connectionId: item.goal.connection_id,
      slug: item.goal_slug ?? item.goal.native_id,
      title: item.goal_title ?? '',
    });
  }

  return {
    declared,
    /** The timeline as shown: remote target tickets carry the labels read for them (HS2-HDHT5E). */
    timeline: labelled,
    loading,
    error,
    selectedStep,
    details,
    busy,
    isOrchestrationProject,
    /**
     * HS2-BRRSYM: the project view the open project's type last showed, where re-selecting its tab
     * returns from Notifications or Settings.
     */
    homeViewMode: (): WorkspaceViewMode =>
      isOrchestrationProject(dependencies.project()?.id)
        ? dependencies.orchestrationViewMode.value
        : dependencies.fallbackViewMode(),
    selected,
    select,
    toggleGroup,
    collapsedGroups,
    planning,
    /** HS2-N5BDE4: the planner's open question, when it asked one. */
    question,
    defaultGoal,
    answerQuestion,
    showPlanner,
    /** HS2-FA8G9J: the planner waits in its terminal and its tab is known, so Show Planner applies. */
    plannerShowable: () => planning.value === 'waiting' && plannerTerminal.value !== undefined,
    /** HS2-5RYXNN: a session an orchestrator started changed; the open project's planner is read again. */
    sessionsChanged: (projectId: string) => {
      const current = dependencies.project();
      if (current?.id === projectId && streamKey) void refreshPlanning(current, generation);
    },
    proposeAvailability: (): OrchestrationProposeAvailability => {
      const current = dependencies.project();
      if (!current) return { available: false };
      if (planning.value === 'planning')
        return { available: false, reason: 'A planning session is working on the plan now' };
      if (planning.value === 'waiting')
        return { available: false, reason: 'The planning session is waiting for an answer in its terminal tab' };
      return planner.availability(current);
    },
    start: () => setAutomatic(true),
    pause: () => setAutomatic(false),
    runControl,
    runValidation,
    linkGate,
    reopen,
    planHealth,
    keepCheckedInPlan,
    stepName,
    overview,
    decisionsDialog,
    showDecisions,
    loadEarlierDecisions: () => {
      if (decisionsDialog.value?.more && !decisionsDialog.value.loading) void readDecisions(true);
    },
    closeDecisions: () => {
      decisionsDialog.value = undefined;
    },
    /** HS2-A4RVYA: the Use Template… dialog and its actions. */
    templateDialog,
    openTemplates: () => openTemplates(),
    selectTemplate: (templateId: string) => {
      selectTemplate(templateId);
    },
    editTemplateInput,
    chooseTemplateStep,
    applyTemplate,
    openSaveTemplate,
    editSaveTemplate,
    cancelSaveTemplate,
    submitSaveTemplate,
    openTemplateGoal,
    /** On a phone, the back control returns from a template's detail to the list. */
    backToTemplates: () => {
      const state = templateDialog.value;
      if (state) templateDialog.value = { ...state, detailActive: false };
    },
    closeTemplates: () => {
      if (previewTimer !== undefined) clearTimeout(previewTimer);
      previewTimer = undefined;
      templateDialog.value = undefined;
      applyOperation = undefined;
    },
    propose,
    updatePlan,
    cancelPlanUpdate,
    /** HS2-190RDX: whether Update Plan can be pressed now, and why not. */
    updateAvailability: (): OrchestrationProposeAvailability => {
      const current = dependencies.project();
      if (!current) return { available: false };
      if (planning.value === 'waiting')
        return { available: false, reason: 'The planning session is waiting for an answer in its terminal tab' };
      return planner.availability(current);
    },
    showRequest,
    openGoal,
    chooseProjects: () => {
      dependencies.openOrchestrationSettings();
    },
    placeTimeline,
    dispose() {
      if (previewTimer !== undefined) clearTimeout(previewTimer);
      follows.abort();
      streamStop?.();
      eventsStop?.();
      labelReader?.reset();
      if (typeof cancelAnimationFrame === 'function') placement.cancel();
      observed?.observer.disconnect();
      for (const dispose of disposers.splice(0)) dispose();
    },
  };
}

export type OrchestrationViewController = ReturnType<typeof createOrchestrationViewController>;
