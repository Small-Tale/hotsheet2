/**
 * HS2-J89JC7: the Orchestration view's state and side effects. One genuine long poll of the selected
 * project's timeline while the view is open (version cursor, bounded failure backoff, no interval);
 * otherwise one read per project to learn whether it is an orchestration project, so the view
 * switcher offers the tab. Selection, the inspector's run details, kick-off and run controls live
 * here; the shell renders from these signals.
 */
import { batch, computed, effect, type Signal, signal } from 'kerfjs';

import { Api } from '../api';
import type { OrchestrationInspectorDetails } from '../components/orchestration-inspector';
import type { OrchestrationProposeAvailability } from '../components/orchestration-timeline-view';
import type { WorkspaceViewMode } from '../components/workspace-controls';
import type { ConfirmationOptions } from '../confirmation';
import type { Project } from '../interactions/types';
import { parseBuiltAhead } from '../orchestration-built-ahead';
import { createRemoteLabelReader, type RemoteTicketLabel, withRemoteLabels } from '../orchestration-remote-labels';
import { parseRunReconciliation } from '../orchestration-run-checks';
import type { OrchestrationTimeline } from '../orchestration-timeline';
import { findTimelineItem, startColumnId } from '../orchestration-timeline-board';
import { parseValidationStep, type PendingValidation, stepsToRun } from '../orchestration-validation-steps';
import { collapsedTicketGroupKeys, toggleTicketGroupCollapsed } from '../ticket-group-state';
import type { TicketLinkMatch } from '../ticket-link-resolution';
import { startVersionedLongPoll } from '../versioned-long-poll';

/** The collapse scope of the Orchestration view's project groups (HS2-WZ3JGZ storage). */
export const ORCHESTRATION_GROUP_SCOPE = 'orchestration';

/** The planning session behind Propose Plan (HS2-HCW7MD); the UX demo passes fixture state. */
export interface OrchestrationPlanner {
  availability(project: Project): OrchestrationProposeAvailability;
  propose(project: Project): Promise<string>;
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

const PLANNING_REFUSALS: Record<string, string> = {
  plan_no_targets: 'Add projects in Orchestration settings before proposing a plan.',
  plan_no_goals: 'Write an open goal ticket in this project before proposing a plan.',
  plan_goal_not_open: 'That goal ticket is not open.',
};

/** Propose Plan through the server's planning session, in the tool the person chose (no server default). */
export function createServerPlanner(options: {
  choice: (project: Project) => AiToolChoice | undefined;
  client: (project: Project) => Api;
  personId: () => string;
}): OrchestrationPlanner {
  return {
    availability: (project) =>
      options.choice(project)
        ? { available: true }
        : { available: false, reason: 'Choose an AI tool for this project in Settings first' },
    async propose(project) {
      const choice = options.choice(project);
      if (!choice) throw new Error('Choose an AI tool for this project in Settings first.');
      try {
        const proposal = await options
          .client(project)
          .proposeOrchestrationPlan(
            project.id,
            { ...choice, operation_id: `propose-${crypto.randomUUID().replaceAll('-', '')}` },
            options.personId(),
          );
        return proposal.outcome === 'existing'
          ? 'A planning session is already working on the plan.'
          : 'Planning started. The planning session is in the drawer.';
      } catch (reason) {
        const code = (reason as { code?: string } | undefined)?.code;
        const known = code ? PLANNING_REFUSALS[code] : undefined;
        if (known) throw new Error(known, { cause: reason });
        throw reason;
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
  readonly pendingOrchestratorRequests?: () => ReadonlySet<string>;
}

const message = (reason: unknown) => (reason instanceof Error ? reason.message : String(reason));

/** The selected run's records as the inspector shows them; an unread record leaves its part absent. */
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
        state: text(gate.state) ?? 'unknown',
        provider: text(gate.provider),
        commit: text(gate.commit),
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
  /** HS2-9XJEZC: Run Validation requests waiting on a person, by `<step id>:<validation step id>`. */
  const pendingValidations = signal<Readonly<Record<string, PendingValidation>>>({});
  /** HS2-HCW7MD: where planning stands (`idle`, `planning`, `ended`), read when the timeline changes. */
  const planning = signal<string | undefined>(undefined);
  const disposers: Array<() => void> = [];
  let streamKey = '',
    streamStop: (() => void) | undefined,
    generation = 0;
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

  // The long poll runs while the view is open.
  disposers.push(
    effect(() => {
      const current = dependencies.project(),
        key = current && dependencies.viewMode.value === 'orchestration' ? `${current.id}:${current.apiPath}` : '';
      if (key === streamKey) return;
      streamStop?.();
      streamStop = undefined;
      labelReader?.reset();
      labelReader = undefined;
      streamKey = key;
      const run = ++generation;
      batch(() => {
        timeline.value = undefined;
        remoteLabels.value = {};
        error.value = undefined;
        selectedStep.value = undefined;
        details.value = undefined;
        loading.value = Boolean(key);
        planning.value = undefined;
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
            learn(current.id, view);
            timeline.value = view;
            loading.value = false;
            error.value = undefined;
            if (selectedStep.value && !findTimelineItem(view, selectedStep.value)) selectedStep.value = undefined;
          });
          reader.fill(view);
          placeTimeline();
          void refreshDetails();
          void refreshPlanning(current, run);
        },
        onError: (reason) => {
          if (run !== generation) return;
          batch(() => {
            loading.value = false;
            error.value = message(reason);
          });
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
      if (run === generation) planning.value = typeof view.state === 'string' ? view.state : undefined;
    } catch {
      if (run === generation) planning.value = undefined;
    }
  }

  const selected = () => findTimelineItem(labelled.value, selectedStep.value)?.item;

  let detailsRequest = 0;
  async function refreshDetails() {
    const current = dependencies.project(),
      item = selected(),
      run = item?.run;
    const request = ++detailsRequest;
    if (!current || !item || !run) {
      details.value = undefined;
      return;
    }
    details.value = { ...(details.value ?? {}), loading: true };
    const client = clientFor(current);
    const [gates, events, validations, record, readiness] = await Promise.allSettled([
      client.orchestrationRunGates(current.id, run.run_id),
      client.orchestrationRunEvents(current.id, run.run_id),
      client.orchestrationValidations(current.id, item.goal),
      client.orchestrationRun(current.id, run.run_id),
      client.orchestrationReadiness(current.id, item.step_id),
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
      const shown = dependencies.pendingOrchestratorRequests?.();
      const waiting = Object.values(pendingValidations.value);
      if (!shown || waiting.length === 0) return;
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
   * is final, which is how the orchestrator collects a result (§26.12).
   */
  async function sendValidations(requests: readonly PendingValidation[], announce: boolean) {
    const current = dependencies.project(),
      item = selected();
    if (!current || !item) return;
    const client = clientFor(current),
      run = generation;
    const call = (entry: PendingValidation, waitMs: number) =>
      client.orchestrationValidationRun(current.id, {
        ticket: item.goal,
        step_id: entry.step_id,
        operation_id: entry.operation_id,
        run_id: item.run?.run_id,
        wait_ms: waitMs,
      });
    const unsettled = (answer: Record<string, unknown>) => {
      const result = answer.result as { state?: unknown } | undefined;
      return result?.state === 'running' || (typeof answer.attempts_left === 'number' && answer.attempts_left > 0);
    };
    const follow = async (entry: PendingValidation) => {
      // At most an hour of eight-second waits: the longest a step may run (§26.12), and a bound.
      for (let wait = 0; wait < 450 && run === generation; wait += 1) {
        try {
          const answer = await call(entry, 8_000);
          void refreshDetails();
          if (!unsettled(answer)) return;
        } catch {
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
        const refusal = reason as { code?: string; requestId?: string };
        if (refusal.code === 'permission_required' && refusal.requestId) {
          if (!next.has(key)) asked += 1;
          next.set(key, { ...entry, request_id: refusal.requestId });
        } else {
          next.delete(key);
          if (refusal.code === 'permission_denied') denied += 1;
          else failed ||= message(reason);
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
    const waiting = new Set(Object.values(pendingValidations.value).map((entry) => entry.step_id));
    const requests = stepsToRun(steps)
      .filter((step) => !waiting.has(step.step_id))
      .map((step) => ({
        step_id: step.step_id,
        operation_id: `validate-${item.step_id}-${step.step_id}-${crypto.randomUUID().replaceAll('-', '').slice(0, 12)}`,
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

  /** Start or pause by turning the policy's automatic placement on or off (HS2-ECWG50). */
  async function setAutomatic(automatic: boolean) {
    const current = dependencies.project();
    if (!current || busy.value) return;
    busy.value = true;
    try {
      const client = clientFor(current),
        stored = await client.orchestrationPolicy(current.id);
      if (!stored.policy) {
        dependencies.showToast('Set the orchestration limits in Orchestration settings first.');
        return;
      }
      const placement = { ...(stored.policy.placement ?? {}), automatic };
      await client.setOrchestrationPolicy(current.id, stored.revision, { ...stored.policy, placement });
      dependencies.showToast(automatic ? 'Orchestration started.' : 'Orchestration paused. Running work continues.');
    } catch (reason) {
      dependencies.showToast(`${automatic ? 'Start' : 'Pause'} failed: ${message(reason)}`);
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
      dependencies.showToast(`The run could not be changed: ${message(reason)}`);
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
      dependencies.showToast(`The work could not be reopened: ${message(reason)}`);
    } finally {
      busy.value = false;
    }
    void refreshDetails();
  }

  async function propose() {
    const current = dependencies.project();
    if (!current || busy.value || !planner.availability(current).available) return;
    busy.value = true;
    try {
      dependencies.showToast(await planner.propose(current));
      planning.value = 'planning';
    } catch (reason) {
      dependencies.showToast(`Propose Plan failed: ${message(reason)}`);
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
    selected,
    select,
    toggleGroup,
    collapsedGroups,
    planning,
    proposeAvailability: (): OrchestrationProposeAvailability => {
      const current = dependencies.project();
      if (!current) return { available: false };
      if (planning.value === 'planning')
        return { available: false, reason: 'A planning session is working on the plan now' };
      return planner.availability(current);
    },
    start: () => setAutomatic(true),
    pause: () => setAutomatic(false),
    runControl,
    runValidation,
    reopen,
    propose,
    showRequest,
    openGoal,
    chooseProjects: () => {
      dependencies.openOrchestrationSettings();
    },
    placeTimeline,
    dispose() {
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
