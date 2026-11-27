import { signal } from 'kerfjs';
import { describe, expect, it, vi } from 'vitest';

import { type Api, ApiHttpError } from '../api';
import type { WorkspaceViewMode } from '../components/workspace-controls';
import type { Project } from '../interactions/types';
import type { OrchestrationTimeline } from '../orchestration-timeline';
import { demoTimeline } from '../ux-demo/orchestration-fixtures';
import type { OrchestrationProjectViewMode } from '../workspace-view-memory';
import {
  coalescedPlacement,
  createOrchestrationViewController,
  createServerPlanner,
  editTriggerForm,
  orchestrationPersonId,
  parseOrchestrationDetails,
  plannerTerminalOf,
  planningDefaultGoal,
  planningPhase,
  planningQuestion,
  startOrchestrationTimelineStream,
} from './orchestration-view';

const versioned = (version: string): OrchestrationTimeline => ({ ...demoTimeline('running'), version });

describe('the timeline long poll (HS2-J89JC7)', () => {
  it('passes the version it last saw, reports each new version once, and backs off after failures', async () => {
    const asked: Array<string | undefined> = [];
    const answers: Array<OrchestrationTimeline | Error> = [
      versioned('v1'),
      versioned('v1'),
      new Error('offline'),
      new Error('offline'),
      versioned('v2'),
    ];
    const delays: number[] = [];
    const seen: string[] = [];
    const errors: string[] = [];
    let finish: () => void = () => undefined;
    const exhausted = new Promise<void>((resolve) => {
      finish = resolve;
    });
    const stop = startOrchestrationTimelineStream({
      client: {
        orchestrationTimeline: (_checkout: string, after?: string) => {
          asked.push(after);
          const next = answers.shift();
          if (!next) {
            finish();
            return new Promise<OrchestrationTimeline>(() => undefined);
          }
          return next instanceof Error ? Promise.reject(next) : Promise.resolve(next);
        },
      },
      checkout: 'launch-0123456789ab',
      onTimeline: (timeline) => seen.push(timeline.version),
      onError: (error) => errors.push((error as Error).message),
      delay: (ms) => {
        delays.push(ms);
        return Promise.resolve();
      },
    });
    await exhausted;
    stop();
    expect(seen).toEqual(['v1', 'v2']);
    expect(errors).toEqual(['offline', 'offline']);
    // The unchanged v1 came back at once, so it backs off like a failure (HS2-YA6D31).
    expect(delays).toEqual([1_000, 2_000, 4_000]);
    // The first read has no version; a failure forgets it, so the next read answers at once.
    expect(asked).toEqual([undefined, 'v1', 'v1', undefined, undefined, 'v2']);
  });

  it('stops reading when stopped, even with a read in flight', async () => {
    let resolveRead: (value: OrchestrationTimeline) => void = () => undefined;
    const onTimeline = vi.fn();
    const stop = startOrchestrationTimelineStream({
      client: {
        orchestrationTimeline: () =>
          new Promise<OrchestrationTimeline>((resolve) => {
            resolveRead = resolve;
          }),
      },
      checkout: 'launch-0123456789ab',
      onTimeline,
      onError: vi.fn(),
    });
    stop();
    resolveRead(versioned('late'));
    await Promise.resolve();
    expect(onTimeline).not.toHaveBeenCalled();
  });
});

describe('Propose Plan through the planning session (HS2-HCW7MD seam, HS2-J89JC7)', () => {
  const project = { id: 'launch-0123456789ab', name: 'Launch', apiPath: '/api' } as Project;

  it('needs the tool the person chose, sends it with a fresh operation id, and explains refusals', async () => {
    const propose = vi.fn();
    const planner = createServerPlanner({
      choice: (current) => (current.id === project.id ? { tool: 'claude', model: 'model-a' } : undefined),
      client: () => ({ proposeOrchestrationPlan: propose }) as never,
      personId: () => 'person-1',
    });
    expect(planner.availability(project)).toEqual({ available: true });
    expect(planner.availability({ ...project, id: 'other' }).available).toBe(false);
    propose.mockResolvedValueOnce({ outcome: 'started' });
    expect(await planner.propose(project)).toMatch(/drawer/);
    expect(propose).toHaveBeenCalledWith(
      project.id,
      expect.objectContaining({ tool: 'claude', model: 'model-a', operation_id: expect.stringMatching(/^propose-/) }),
      'person-1',
    );
    propose.mockResolvedValueOnce({ outcome: 'existing' });
    expect(await planner.propose(project)).toMatch(/already working/);
    // HS2-FDS95D: a planner that finished its turn was given the new proposal.
    propose.mockResolvedValueOnce({ outcome: 'replanned' });
    expect(await planner.propose(project)).toMatch(/planning again/);
    propose.mockRejectedValueOnce(new ApiHttpError('no goals', 409, 'plan_no_goals'));
    await expect(planner.propose(project)).rejects.toThrow(/Git ticket source/);
    propose.mockRejectedValueOnce(new ApiHttpError('boom', 500, 'internal'));
    await expect(planner.propose(project)).rejects.toThrow('boom');
    await expect(planner.propose({ ...project, id: 'other' })).rejects.toThrow(/Choose an AI tool/);
    // HS2-2Z4M6M, HS2-8Z9VHH: a refused admission names the machine's count and limit, and where to change it.
    propose.mockRejectedValueOnce(
      new ApiHttpError('raw', 409, 'session_admission_refused', undefined, {
        reason: 'session_cap_reached',
        setting: 'max_sessions',
        limit: 2,
        observed: 2,
      }),
    );
    await expect(planner.propose(project)).rejects.toThrow(
      'The machine already runs 2 AI sessions; its limit is 2. Close a session, or raise Sessions per Machine in Settings › Orchestration › Machine Defaults. (session_cap_reached)',
    );
    // An older host sends the code alone: still words, never the bare code.
    propose.mockRejectedValueOnce(new ApiHttpError('raw', 409, 'session_admission_refused'));
    await expect(planner.propose(project)).rejects.toThrow(/^The machine refused to start another AI session\./);
  });

  it('updates the plan beside the running one, cancels it, and explains refusals (HS2-190RDX)', async () => {
    const update = vi.fn();
    const cancel = vi.fn();
    const planner = createServerPlanner({
      choice: (current) => (current.id === project.id ? { tool: 'claude' } : undefined),
      client: () => ({ updateOrchestrationPlan: update, cancelOrchestrationPlanUpdate: cancel }) as never,
      personId: () => 'person-1',
    });
    update.mockResolvedValueOnce({ outcome: 'started', state: 'drafting' });
    expect(await planner.update(project)).toMatch(/Work continues on the current plan/);
    expect(update).toHaveBeenCalledWith(
      project.id,
      expect.objectContaining({ tool: 'claude', operation_id: expect.stringMatching(/^update-/) }),
      'person-1',
    );
    update.mockResolvedValueOnce({ outcome: 'existing', state: 'drafting' });
    expect(await planner.update(project)).toMatch(/already being updated/);
    update.mockRejectedValueOnce(new ApiHttpError('busy', 409, 'plan_update_planner_busy'));
    await expect(planner.update(project)).rejects.toThrow(/still working/);
    await expect(planner.update({ ...project, id: 'other' })).rejects.toThrow(/Choose an AI tool/);
    cancel.mockResolvedValueOnce({ state: 'cancelled' });
    expect(await planner.cancelUpdate(project)).toMatch(/current plan is unchanged/);
    expect(cancel).toHaveBeenCalledWith(project.id, 'person-1');
    cancel.mockRejectedValueOnce(new ApiHttpError('gone', 409, 'plan_update_not_open'));
    await expect(planner.cancelUpdate(project)).rejects.toThrow(/No plan update/);
  });

  it('says to install a tool, not to choose one, when this machine has none (HS2-N3HWP9)', async () => {
    let installed = false;
    const planner = createServerPlanner({
      choice: () => undefined,
      installed: () => installed,
      client: () => ({}) as never,
      personId: () => 'person-1',
    });
    expect(planner.availability(project)).toEqual({
      available: false,
      reason: 'Install and sign in to an AI tool on this machine first',
    });
    await expect(planner.propose(project)).rejects.toThrow(/Install and sign in/);
    await expect(planner.update(project)).rejects.toThrow(/Install and sign in/);
    // A tool is installed but the one chosen here is not: choosing another in Settings helps.
    installed = true;
    expect(planner.availability(project).reason).toMatch(/Choose an AI tool/);
  });

  it('names this browser’s person with one stable, bounded id', () => {
    const values = new Map<string, string>();
    const storage = {
      getItem: (key: string) => values.get(key) ?? null,
      setItem: (key: string, value: string) => values.set(key, value),
    };
    const first = orchestrationPersonId(storage);
    expect(first).toMatch(/^person-[a-f0-9]{32}$/);
    expect(orchestrationPersonId(storage)).toBe(first);
    values.set('hotsheet.orchestrationPersonId', 'bad id with spaces');
    expect(orchestrationPersonId(storage)).toMatch(/^person-/);
    expect(orchestrationPersonId(undefined)).toMatch(/^person-/);
  });
});

describe('where planning stands (HS2-5RYXNN)', () => {
  it('reads a planner whose tool has not reported its start as waiting, and nothing else', () => {
    const view = (state: unknown, toolReady?: unknown) => ({
      state,
      session: toolReady === undefined ? {} : { tool_ready: toolReady },
    });
    expect(planningPhase(undefined)).toBeUndefined();
    expect(planningPhase({})).toBeUndefined();
    expect(planningPhase(view('idle'))).toBe('idle');
    expect(planningPhase(view('planning'))).toBe('planning');
    expect(planningPhase(view('planning', true))).toBe('planning');
    expect(planningPhase(view('planning', false))).toBe('waiting');
    expect(planningPhase({ state: 'planning' })).toBe('planning');
    // An ended planner is not waiting, whatever its last reading said.
    expect(planningPhase(view('ended', false))).toBe('ended');
    expect(planningPhase(view(7, false))).toBeUndefined();
  });

  it('reads a planner that finished its turn as replannable, unless it waits on its tool (HS2-FDS95D)', () => {
    expect(planningPhase({ state: 'planning', replannable: true, session: { tool_ready: true } })).toBe('replannable');
    expect(planningPhase({ state: 'planning', replannable: true, session: {} })).toBe('replannable');
    expect(planningPhase({ state: 'planning', replannable: false, session: {} })).toBe('planning');
    expect(planningPhase({ state: 'planning', replannable: true, session: { tool_ready: false } })).toBe('waiting');
    // HS2-190RDX: a drafting plan update reads as updating, after a waiting tool.
    expect(planningPhase({ state: 'planning', replannable: true, update: { state: 'drafting' } })).toBe('updating');
    expect(planningPhase({ state: 'planning', session: { tool_ready: false }, update: { state: 'drafting' } })).toBe(
      'waiting',
    );
    expect(planningPhase({ state: 'planning', update: { state: 'applied' } })).toBe('planning');
    expect(planningPhase({ state: 'ended', update: { state: 'drafting' } })).toBe('ended');
    // HS2-N5BDE4: a planner that asked the person shows it, live or ended; a waiting tool comes first.
    const questions = [{ connection_id: 'tickets', native_id: 'G1', slug: 'LCH-1', question: 'N1' }];
    expect(planningPhase({ state: 'planning', questions })).toBe('asked');
    expect(planningPhase({ state: 'ended', questions })).toBe('asked');
    expect(planningPhase({ state: 'planning', questions, update: { state: 'drafting' } })).toBe('asked');
    expect(planningPhase({ state: 'planning', questions, session: { tool_ready: false } })).toBe('waiting');
    expect(planningPhase({ state: 'idle', questions })).toBe('idle');
    expect(planningPhase({ state: 'planning', questions: [] })).toBe('planning');
    expect(planningQuestion({ questions })).toEqual({
      connection_id: 'tickets',
      native_id: 'G1',
      slug: 'LCH-1',
      note_id: 'N1',
    });
    expect(planningQuestion({ questions: [{ native_id: 7 }] })).toBeUndefined();
    expect(planningQuestion({ questions: [{ connection_id: 'c', native_id: 'G2' }] })).toEqual({
      connection_id: 'c',
      native_id: 'G2',
      slug: 'G2',
      note_id: undefined,
    });
    expect(planningQuestion(undefined)).toBeUndefined();
    // HS2-YSRT6Y: `plan show` says only the default goal applies.
    expect(planningDefaultGoal({ default_goal: true })).toBe(true);
    expect(planningDefaultGoal({ default_goal: false })).toBe(false);
    expect(planningDefaultGoal({ default_goal: 'yes' })).toBe(false);
    expect(planningDefaultGoal(undefined)).toBe(false);
    expect(planningPhase({ state: 'ended', replannable: true })).toBe('ended');
  });
});

describe('placing the timeline after a resize (HS2-PJ3SE6)', () => {
  const frames = () => {
    const pending = new Map<number, () => void>();
    let next = 0;
    return {
      schedule: (callback: () => void) => {
        pending.set(++next, callback);
        return next;
      },
      cancel: (handle: number) => {
        pending.delete(handle);
      },
      flush: () => {
        const run = [...pending.values()];
        pending.clear();
        for (const callback of run) callback();
      },
      size: () => pending.size,
    };
  };

  it('keeps a forced request when a timeline update replaces its frame', () => {
    const clock = frames();
    const calls: boolean[] = [];
    const placement = coalescedPlacement(clock.schedule, clock.cancel, (force) => {
      calls.push(force);
      return true;
    });
    placement.request(true); // the board's width changed
    placement.request(); // a timeline update arrives before the frame runs
    expect(clock.size()).toBe(1);
    clock.flush();
    expect(calls).toEqual([true]);
    // Acted on: the next ordinary request is not forced.
    placement.request();
    clock.flush();
    expect(calls).toEqual([true, false]);
  });

  it('stays forced until a frame can act, and cancels a pending frame', () => {
    const clock = frames();
    const calls: boolean[] = [];
    let ready = false;
    const placement = coalescedPlacement(clock.schedule, clock.cancel, (force) => {
      calls.push(force);
      return ready;
    });
    placement.request(true);
    clock.flush(); // the board is not mounted yet
    placement.request();
    ready = true;
    clock.flush();
    expect(calls).toEqual([true, true]);
    placement.request(true);
    placement.cancel();
    clock.flush();
    expect(calls).toEqual([true, true]);
  });
});

describe('the selected run’s details (HS2-XM692S)', () => {
  it('carries the run record’s reconciliation, and marks an unread record', () => {
    const read = parseOrchestrationDetails(undefined, undefined, undefined, 'api', {
      run_id: 'run-3',
      reconciliation: { at_ms: 9, disposition: 'continue', findings: [] },
    });
    expect(read.runRead).toBe(true);
    expect(read.reconciliation).toEqual({ at_ms: 9, disposition: 'continue', findings: [] });
    const none = parseOrchestrationDetails(undefined, undefined, undefined, 'api', { run_id: 'run-3' });
    expect(none.runRead).toBe(true);
    expect(none.reconciliation).toBeUndefined();
    const unread = parseOrchestrationDetails(undefined, undefined, undefined, 'api');
    expect(unread.runRead).toBe(false);
  });

  it('shows a review gate in the review words and leaves other gates alone (HS2-GD3NNK)', () => {
    const gate = (kind: string, state: string, superseded?: string) => ({
      kind,
      state,
      provider: kind,
      commit: 'a'.repeat(40),
      ...(superseded ? { superseded_by: superseded } : {}),
    });
    const read = parseOrchestrationDetails(
      {
        ci: 'passed',
        gates: [
          gate('ci', 'passed'),
          gate('review', 'awaiting_approval', 'next'),
          gate('review', 'failed'),
          gate('deployment', 'awaiting_approval'),
        ],
      },
      undefined,
      undefined,
      'api',
    );
    expect(read.gates?.map((row) => [row.kind, row.state])).toEqual([
      ['ci', 'passed'],
      ['review', 'changes_requested'],
      ['deployment', 'awaiting_approval'],
    ]);
    for (const [state, words] of [
      ['awaiting_approval', 'requested'],
      ['passed', 'approved'],
      ['unknown', 'unknown'],
    ])
      expect(
        parseOrchestrationDetails({ gates: [gate('review', state)] }, undefined, undefined, 'api').gates?.[0].state,
      ).toBe(words);
    // HS2-507DGZ: an unknown CI gate says why its watch is not running; another reason keeps the state.
    const ci = (reason?: string) =>
      parseOrchestrationDetails(
        { gates: [{ ...gate('ci', 'unknown'), ...(reason ? { reason } : {}) }] },
        undefined,
        undefined,
        'api',
      ).gates?.[0].state;
    expect(ci('watch_gone')).toBe(
      'unknown: the CI provider kept ending the watch; check the provider’s credentials and repository, then link the gate again',
    );
    expect(ci('watch_ended')).toBe('unknown: the CI provider ended the watch; it is linked again automatically');
    expect(ci('not_watched')).toBe('unknown: nothing watches it yet; link the gate again to start its watch');
    expect(ci('watch_lost')).toBe('unknown: the watch was interrupted; it resumes when the job monitor runs again');
    expect(ci('evidence_missing')).toBe('unknown');
    expect(ci()).toBe('unknown');
  });

  it('carries the Link Again request of an unknown CI gate whose watch a link starts again (HS2-A7YCB2)', () => {
    const commit = 'b'.repeat(40);
    const read = parseOrchestrationDetails(
      {
        gates: [
          {
            gate_id: 'gone',
            kind: 'ci',
            provider: 'github-actions',
            commit,
            ci: { repository: 'acme/synthetic', workflow: 'CI', branch: 'main' },
            state: 'unknown',
            reason: 'watch_gone',
          },
          { gate_id: 'watched', kind: 'ci', provider: 'github-actions', commit, state: 'pending' },
        ],
      },
      undefined,
      undefined,
      'api',
    );
    expect(read.gates?.map((row) => [row.gate_id, row.relink])).toEqual([
      ['gone', { commit, provider: 'github-actions', repository: 'acme/synthetic', workflow: 'CI', branch: 'main' }],
      ['watched', undefined],
    ]);
  });

  it('gives each Gate event its gate state and its gate’s kind, superseded gates included (HS2-7NJN7S)', () => {
    const event = (at_ms: number, gate_id: string | undefined, gate_state?: string) => ({
      record: {
        at_ms,
        kind: 'gate',
        source: 'target',
        ...(gate_id ? { gate_id } : {}),
        ...(gate_state ? { gate_state } : {}),
      },
    });
    const read = parseOrchestrationDetails(
      {
        gates: [
          { gate_id: 'old', kind: 'ci', state: 'superseded', superseded_by: 'new' },
          { gate_id: 'new', kind: 'ci', state: 'passed' },
          { gate_id: 'review', kind: 'review', state: 'awaiting_approval' },
        ],
      },
      {
        entries: [
          event(1, 'old', 'pending'),
          event(2, 'new', 'passed'),
          event(3, 'review', 'awaiting_approval'),
          event(4, 'unlisted', 'failed'),
          event(5, undefined),
        ],
      },
      undefined,
      'api',
    );
    expect(read.events?.map((row) => [row.at_ms, row.gate_kind, row.gate_state])).toEqual([
      [5, undefined, undefined],
      [4, undefined, 'failed'],
      [3, 'review', 'awaiting_approval'],
      [2, 'ci', 'passed'],
      [1, 'ci', 'pending'],
    ]);
    expect(read.gates?.map((row) => row.gate_id)).toEqual(['new', 'review']);
  });
});

describe('Link Again through the view (HS2-A7YCB2)', () => {
  const commit = 'c'.repeat(40);
  function setup(link: (body: Record<string, unknown>) => Promise<unknown>) {
    const project = { id: 'orch', name: 'orch', root: '/synthetic/orch', apiPath: '/api/orch' } as Project;
    const toasts: string[] = [],
      linked: Array<{ checkout: string; run: string; body: Record<string, unknown>; actor: string }> = [];
    let state = 'unknown';
    const known: Record<string, unknown> = {
      orchestrationTimeline: (_owner: string, after?: string) =>
        after ? new Promise<never>(() => undefined) : Promise.resolve(demoTimeline('running')),
      orchestrationRunGates: () =>
        Promise.resolve({
          gates: [
            {
              gate_id: 'gate-9',
              kind: 'ci',
              provider: 'github-actions',
              commit,
              ci: { repository: 'acme/synthetic', workflow: 'CI', branch: 'main' },
              state,
              ...(state === 'unknown' ? { reason: 'watch_gone' } : {}),
            },
          ],
        }),
      orchestrationRunEvents: () => Promise.resolve({ entries: [] }),
      orchestrationRun: () => Promise.resolve({ run_id: 'run-3' }),
      orchestrationValidations: () => Promise.resolve([]),
      orchestrationReadiness: () => Promise.resolve({}),
      orchestrationSchedulerExplain: () => Promise.resolve({}),
      orchestrationGateLink: (checkout: string, run: string, body: Record<string, unknown>, actor: string) => {
        linked.push({ checkout, run, body, actor });
        return link(body).then((answer) => {
          state = 'pending';
          return answer;
        });
      },
    };
    const client = new Proxy(known, {
      get: (target, key: string) => target[key] ?? (() => new Promise<never>(() => undefined)),
    }) as unknown as Api;
    const controller = createOrchestrationViewController({
      project: () => project,
      viewMode: signal<WorkspaceViewMode>('orchestration'),
      fallbackViewMode: () => 'list',
      orchestrationViewMode: signal<OrchestrationProjectViewMode>('orchestration'),
      mobile: () => false,
      confirm: () => Promise.resolve(false),
      showToast: (message) => void toasts.push(message),
      openTicket: () => Promise.resolve(),
      showPermissionRequest: () => Promise.resolve(),
      openOrchestrationSettings: () => undefined,
      persistWorkspacePreferences: () => undefined,
      planner: { propose: () => Promise.resolve({ ok: false, reason: 'synthetic' }) } as never,
      personId: () => 'person-1',
      createClient: () => client,
      orchestrationProject: () => true,
    });
    return { controller, toasts, linked };
  }

  it('links the selected run’s gate with its own scope as the person, then reads the gate again', async () => {
    const t = setup(() => Promise.resolve({ gate: { state: 'pending' } }));
    await vi.waitFor(() => {
      expect(t.controller.timeline.value).toBeDefined();
    });
    t.controller.select('step-3');
    await vi.waitFor(() => {
      expect(t.controller.details.value?.gates?.[0].relink).toBeDefined();
    });
    // An unknown gate id, or a second press while the first is on its way, sends nothing more.
    await t.controller.linkGate('gate-unknown');
    expect(t.linked).toEqual([]);
    const first = t.controller.linkGate('gate-9');
    expect(t.controller.busy.value).toBe(true);
    await t.controller.linkGate('gate-9');
    await first;
    expect(t.linked).toEqual([
      {
        checkout: 'orch',
        run: 'run-3',
        body: { commit, provider: 'github-actions', repository: 'acme/synthetic', workflow: 'CI', branch: 'main' },
        actor: 'person-1',
      },
    ]);
    expect(t.controller.busy.value).toBe(false);
    // The gate watched again reads pending, and Link Again is gone.
    expect(t.controller.details.value?.gates?.[0]).toMatchObject({ state: 'pending', relink: undefined });
    await t.controller.linkGate('gate-9');
    expect(t.linked).toHaveLength(1);
    expect(t.toasts).toEqual([]);
    t.controller.dispose();
  });

  it('says why when the link is refused, and leaves the gate to be linked again', async () => {
    const t = setup(() => Promise.reject(new ApiHttpError('repository mismatch', 422, 'gate_repository_mismatch')));
    await vi.waitFor(() => {
      expect(t.controller.timeline.value).toBeDefined();
    });
    t.controller.select('step-3');
    await vi.waitFor(() => {
      expect(t.controller.details.value?.gates?.[0].relink).toBeDefined();
    });
    await t.controller.linkGate('gate-9');
    expect(t.toasts).toHaveLength(1);
    expect(t.toasts[0]).toMatch(/^The gate could not be linked again: /);
    expect(t.controller.busy.value).toBe(false);
    expect(t.controller.details.value?.gates?.[0].relink).toBeDefined();
    t.controller.dispose();
  });
});

describe('switching between orchestration and software projects (HS2-BRRSYM)', () => {
  const project = (id: string) => ({ id, name: id, root: `/synthetic/${id}`, apiPath: `/api/${id}` }) as Project;
  function setup(initialView: WorkspaceViewMode, types: Record<string, boolean | undefined>) {
    const current = signal<Project | undefined>(undefined),
      viewMode = signal<WorkspaceViewMode>(initialView),
      ticketViewMode = signal<WorkspaceViewMode>(initialView === 'board' ? 'board' : 'list'),
      orchestrationViewMode = signal<OrchestrationProjectViewMode>('orchestration'),
      known = signal(types),
      persisted = vi.fn();
    const pending = () => new Promise<never>(() => undefined);
    const client = new Proxy({}, { get: () => pending }) as unknown as Api;
    const controller = createOrchestrationViewController({
      project: () => current.value,
      viewMode,
      fallbackViewMode: () => ticketViewMode.value,
      orchestrationViewMode,
      mobile: () => false,
      confirm: () => Promise.resolve(false),
      showToast: () => undefined,
      openTicket: () => Promise.resolve(),
      showPermissionRequest: () => Promise.resolve(),
      openOrchestrationSettings: () => undefined,
      persistWorkspacePreferences: persisted,
      planner: { propose: () => Promise.resolve({ ok: false, reason: 'synthetic' }) } as never,
      personId: () => 'person-1',
      createClient: () => client,
      orchestrationProject: (id) => known.value[id],
    });
    // The workspace remembers the ticket view the person last chose in a software project, as the runtime does.
    const choose = (mode: WorkspaceViewMode) => {
      viewMode.value = mode;
      const inOrchestration = controller.isOrchestrationProject(current.value?.id);
      if ((mode === 'list' || mode === 'board' || mode === 'workers') && !inOrchestration) ticketViewMode.value = mode;
    };
    return { current, viewMode, orchestrationViewMode, known, persisted, controller, choose };
  }

  it('returns to the Orchestration view after visiting a software project', () => {
    const t = setup('list', { orch: true, soft: false });
    t.current.value = project('orch');
    expect(t.viewMode.value).toBe('orchestration'); // first visit
    t.current.value = project('soft');
    expect(t.viewMode.value).toBe('list'); // the software project's last ticket view
    t.choose('board');
    t.current.value = project('orch');
    expect(t.viewMode.value).toBe('orchestration');
    t.current.value = project('soft');
    expect(t.viewMode.value).toBe('board');
    expect(t.persisted).toHaveBeenCalled();
    t.controller.dispose();
  });

  it('remembers a ticket view chosen in an orchestration project apart from the software one', () => {
    const t = setup('board', { orch: true, soft: false });
    t.current.value = project('soft');
    t.current.value = project('orch');
    expect(t.viewMode.value).toBe('orchestration');
    t.choose('list');
    expect(t.orchestrationViewMode.value).toBe('list');
    expect(t.controller.homeViewMode()).toBe('list');
    // The software project keeps its own Columns, not the List chosen in the orchestration project.
    t.current.value = project('soft');
    expect(t.viewMode.value).toBe('board');
    expect(t.controller.homeViewMode()).toBe('board');
    t.choose('workers');
    t.choose('board');
    expect(t.orchestrationViewMode.value).toBe('list');
    t.current.value = project('orch');
    expect(t.viewMode.value).toBe('list');
    t.choose('orchestration');
    t.current.value = project('soft');
    expect(t.viewMode.value).toBe('board');
    t.controller.dispose();
  });

  it('waits for an unread project type, unless the person chose a view meanwhile', () => {
    const t = setup('list', { soft: false });
    t.current.value = project('orch');
    expect(t.viewMode.value).toBe('list');
    t.known.value = { soft: false, orch: true };
    expect(t.viewMode.value).toBe('orchestration');
    t.current.value = project('soft');
    t.current.value = project('late');
    t.choose('board');
    t.known.value = { soft: false, orch: true, late: true };
    expect(t.viewMode.value).toBe('board');
    t.controller.dispose();
  });

  it('keeps Notifications or Settings open across a switch to an orchestration project', () => {
    const t = setup('settings', { orch: true, soft: false });
    t.current.value = project('soft');
    t.current.value = project('orch');
    expect(t.viewMode.value).toBe('settings');
    expect(t.orchestrationViewMode.value).toBe('orchestration');
    t.controller.dispose();
  });
});

describe('Start Orchestration with and without a saved policy (HS2-D9KP86, HS2-TZJYKH)', () => {
  const project = { id: 'orch', name: 'orch', root: '/synthetic/orch', apiPath: '/api/orch' } as Project;
  /** A server that edits its stored policy as the runtime's automatic route does. */
  function setup(stored: Record<string, unknown> | null, refuse?: string) {
    const toasts: string[] = [],
      calls: Array<{ owner: string; automatic: boolean }> = [];
    let policy = stored;
    const known: Record<string, unknown> = {
      setOrchestrationAutomatic: (owner: string, automatic: boolean) => {
        calls.push({ owner, automatic });
        if (refuse) return Promise.reject(new Error(refuse));
        const saved_default = policy === null,
          before = policy ?? {};
        policy = { ...before, placement: { ...(before.placement as object | undefined), automatic } };
        return Promise.resolve({ enabled: true, revision: `rev-${String(calls.length)}`, policy, saved_default });
      },
      // The view never reads and writes the whole policy for Start or Pause any more.
      orchestrationPolicy: () => Promise.reject(new Error('read the policy')),
      setOrchestrationPolicy: () => Promise.reject(new Error('wrote the whole policy')),
    };
    // Every other read stays pending, as a server that has not answered yet.
    const client = new Proxy(known, {
      get: (target, key: string) => target[key] ?? (() => new Promise<never>(() => undefined)),
    }) as unknown as Api;
    const controller = createOrchestrationViewController({
      project: () => project,
      viewMode: signal<WorkspaceViewMode>('list'),
      fallbackViewMode: () => 'list',
      orchestrationViewMode: signal<OrchestrationProjectViewMode>('orchestration'),
      mobile: () => false,
      confirm: () => Promise.resolve(false),
      showToast: (message) => void toasts.push(message),
      openTicket: () => Promise.resolve(),
      showPermissionRequest: () => Promise.resolve(),
      openOrchestrationSettings: () => undefined,
      persistWorkspacePreferences: () => undefined,
      planner: { propose: () => Promise.resolve({ ok: false, reason: 'synthetic' }) } as never,
      personId: () => 'person-1',
      createClient: () => client,
      orchestrationProject: () => false,
    });
    return { controller, toasts, calls, policy: () => policy };
  }

  it('says the default policy was saved when the server saved it with the first Start', async () => {
    const t = setup(null);
    await t.controller.start();
    expect(t.calls).toEqual([{ owner: 'orch', automatic: true }]);
    expect(t.toasts).toEqual(['Saved the default policy and started orchestration.']);
    // Paused and started again: the saved policy is edited, nothing else changes.
    await t.controller.pause();
    await t.controller.start();
    expect(t.calls.slice(1)).toEqual([
      { owner: 'orch', automatic: false },
      { owner: 'orch', automatic: true },
    ]);
    expect(t.toasts.slice(1)).toEqual(['Orchestration paused. Running work continues.', 'Orchestration started.']);
    expect(t.policy()).toEqual({ placement: { automatic: true } });
    t.controller.dispose();
  });

  it('starts a saved policy with the plain toast, and repeats are separate calls', async () => {
    const t = setup({ machine: { max_sessions: 4 }, placement: { automatic: false, aging_minutes: 30 } });
    await t.controller.start();
    await t.controller.start();
    expect(t.calls).toEqual([
      { owner: 'orch', automatic: true },
      { owner: 'orch', automatic: true },
    ]);
    expect(t.toasts).toEqual(['Orchestration started.', 'Orchestration started.']);
    expect(t.policy()).toEqual({ machine: { max_sessions: 4 }, placement: { automatic: true, aging_minutes: 30 } });
    t.controller.dispose();
  });

  it('says a refused Start or Pause failed and writes nothing else', async () => {
    const t = setup(null, 'conflict');
    await t.controller.start();
    await t.controller.pause();
    expect(t.calls).toEqual([
      { owner: 'orch', automatic: true },
      { owner: 'orch', automatic: false },
    ]);
    expect(t.toasts.map((toast) => toast.split(':')[0])).toEqual(['Start failed', 'Pause failed']);
    expect(t.policy()).toBeNull();
    t.controller.dispose();
  });
});

describe('Show Planner opens the planning session’s tab (HS2-FA8G9J)', () => {
  it('reads the planner’s terminal from plan show', () => {
    expect(plannerTerminalOf({ session: { session: { checkout_id: 'orch', session_id: 't-1' } } })).toEqual({
      checkout_id: 'orch',
      session_id: 't-1',
    });
    expect(plannerTerminalOf({ session: { session: { checkout_id: 'orch' } } })).toBeUndefined();
    expect(plannerTerminalOf({})).toBeUndefined();
    expect(plannerTerminalOf(undefined)).toBeUndefined();
  });

  function setup(view: Record<string, unknown>, tabs: string[]) {
    const project = { id: 'orch', name: 'orch', root: '/synthetic/orch', apiPath: '/api/orch' } as Project;
    const toasts: string[] = [],
      shown: string[] = [];
    let planningView = view;
    const known: Record<string, unknown> = {
      orchestrationPlanning: () => Promise.resolve(planningView),
      // The first read answers; the long poll after it waits, as an unchanged server does.
      orchestrationTimeline: (_owner: string, after?: string) =>
        after ? new Promise<never>(() => undefined) : Promise.resolve(demoTimeline('no_plan')),
    };
    const client = new Proxy(known, {
      get: (target, key: string) => target[key] ?? (() => new Promise<never>(() => undefined)),
    }) as unknown as Api;
    const controller = createOrchestrationViewController({
      project: () => project,
      viewMode: signal<WorkspaceViewMode>('orchestration'),
      fallbackViewMode: () => 'list',
      orchestrationViewMode: signal<OrchestrationProjectViewMode>('orchestration'),
      mobile: () => false,
      confirm: () => Promise.resolve(false),
      showToast: (message) => void toasts.push(message),
      openTicket: () => Promise.resolve(),
      showPermissionRequest: () => Promise.resolve(),
      openOrchestrationSettings: () => undefined,
      persistWorkspacePreferences: () => undefined,
      planner: { propose: () => Promise.resolve({ ok: false, reason: 'synthetic' }) } as never,
      personId: () => 'person-1',
      createClient: () => client,
      orchestrationProject: () => true,
      showTerminal: (projectId, terminalId) => {
        const key = `${projectId}:${terminalId}`;
        if (!tabs.includes(key)) return false;
        shown.push(key);
        return true;
      },
    });
    return {
      controller,
      toasts,
      shown,
      replan: (next: Record<string, unknown>) => {
        planningView = next;
        controller.sessionsChanged('orch');
      },
    };
  }
  const waiting = (checkout = 'orch') => ({
    state: 'planning',
    session: { tool_ready: false, session: { checkout_id: checkout, session_id: 'planner-1' } },
  });

  it('opens the drawer on the planner’s tab while it waits, and says when this window does not hold it', async () => {
    const t = setup(waiting(), ['orch:planner-1']);
    await vi.waitFor(() => {
      expect(t.controller.plannerShowable()).toBe(true);
    });
    t.controller.showPlanner();
    expect(t.shown).toEqual(['orch:planner-1']);
    expect(t.toasts).toEqual([]);
    // Ready again: nothing to show.
    t.replan({ state: 'planning', session: { tool_ready: true, session: waiting().session.session } });
    await vi.waitFor(() => {
      expect(t.controller.plannerShowable()).toBe(false);
    });
    t.controller.dispose();
  });

  it('falls back to the open project’s tab, then says the terminal is not open here', async () => {
    const t = setup(waiting('elsewhere'), ['orch:planner-1']);
    await vi.waitFor(() => {
      expect(t.controller.plannerShowable()).toBe(true);
    });
    t.controller.showPlanner();
    expect(t.shown).toEqual(['orch:planner-1']);
    const none = setup(waiting('elsewhere'), []);
    await vi.waitFor(() => {
      expect(none.controller.plannerShowable()).toBe(true);
    });
    none.controller.showPlanner();
    expect(none.toasts).toEqual(['The planning session’s terminal is not open in this window.']);
    t.controller.dispose();
    none.controller.dispose();
  });
});

describe('following a running validation (HS2-JGFPG4)', () => {
  type Answer = Record<string, unknown> | Error;
  const running = { result: { state: 'running' }, attempts_left: 0 };
  const passed = { result: { state: 'passed' }, attempts_left: 0 };
  function setup(answers: Answer[], options: { holdDelays?: boolean } = {}) {
    const project = { id: 'orch', name: 'orch', root: '/synthetic/orch', apiPath: '/api/orch' } as Project;
    const toasts: string[] = [],
      waits: number[] = [],
      delays: number[] = [],
      held: Array<() => void> = [];
    const viewMode = signal<WorkspaceViewMode>('orchestration');
    const known: Record<string, unknown> = {
      orchestrationTimeline: (_owner: string, after?: string) =>
        after ? new Promise<never>(() => undefined) : Promise.resolve(demoTimeline('running')),
      orchestrationRunGates: () => Promise.resolve({ gates: [] }),
      orchestrationRunEvents: () => Promise.resolve({ entries: [] }),
      orchestrationRun: () => Promise.resolve({ run_id: 'run-3' }),
      orchestrationValidations: () => Promise.resolve([{ state: 'failed', step: { definition: { step_id: 'unit' } } }]),
      orchestrationReadiness: () => Promise.resolve({}),
      orchestrationSchedulerExplain: () => Promise.resolve({}),
      orchestrationValidationRun: (_checkout: string, body: { wait_ms: number }) => {
        waits.push(body.wait_ms);
        const next = answers.shift() ?? passed;
        return next instanceof Error ? Promise.reject(next) : Promise.resolve(next);
      },
    };
    const client = new Proxy(known, {
      get: (target, key: string) => target[key] ?? (() => new Promise<never>(() => undefined)),
    }) as unknown as Api;
    const controller = createOrchestrationViewController({
      project: () => project,
      viewMode,
      fallbackViewMode: () => 'list',
      orchestrationViewMode: signal<OrchestrationProjectViewMode>('orchestration'),
      mobile: () => false,
      confirm: () => Promise.resolve(false),
      showToast: (message) => void toasts.push(message),
      openTicket: () => Promise.resolve(),
      showPermissionRequest: () => Promise.resolve(),
      openOrchestrationSettings: () => undefined,
      persistWorkspacePreferences: () => undefined,
      planner: { propose: () => Promise.resolve({ ok: false, reason: 'synthetic' }) } as never,
      personId: () => 'person-1',
      createClient: () => client,
      orchestrationProject: () => true,
      delay: (ms, abort) => {
        delays.push(ms);
        if (!options.holdDelays) return Promise.resolve();
        return new Promise<void>((resolve) => {
          held.push(resolve);
          abort.addEventListener(
            'abort',
            () => {
              resolve();
            },
            { once: true },
          );
        });
      },
    });
    async function start() {
      await vi.waitFor(() => {
        expect(controller.timeline.value).toBeDefined();
      });
      controller.select('step-3');
      await vi.waitFor(() => {
        expect(controller.details.value?.validations?.map((step) => step.step_id)).toEqual(['unit']);
      });
      await controller.runValidation();
    }
    return { controller, toasts, waits, delays, held, viewMode, start };
  }

  it('asks again on the bounded backoff after a dropped request, then follows to the result', async () => {
    const t = setup([
      running,
      new TypeError('network'),
      new ApiHttpError('busy', 503),
      running,
      new ApiHttpError('timeout', 408),
      passed,
    ]);
    await t.start();
    await vi.waitFor(() => {
      expect(t.waits).toEqual([0, 8_000, 8_000, 8_000, 8_000, 8_000]);
    });
    // The failures before an answer back off 1 s then 2 s; an answer resets the count.
    expect(t.delays).toEqual([1_000, 2_000, 1_000]);
    expect(t.toasts).toEqual([]);
    t.controller.dispose();
  });

  it('stops after repeated failures and says so once', async () => {
    const t = setup([running, ...Array.from({ length: 6 }, () => new TypeError('network'))]);
    await t.start();
    await vi.waitFor(() => {
      expect(t.toasts).toEqual([
        'Validation step unit is no longer followed: The Hot Sheet server could not be reached.',
      ]);
    });
    expect(t.waits).toEqual([0, 8_000, 8_000, 8_000, 8_000, 8_000]);
    expect(t.delays).toEqual([1_000, 2_000, 4_000, 8_000]);
    t.controller.dispose();
  });

  it('does not ask again after a refusal, and names it', async () => {
    const t = setup([running, new ApiHttpError('no such step', 404, 'validation_step_unknown')]);
    await t.start();
    await vi.waitFor(() => {
      expect(t.toasts).toHaveLength(1);
    });
    expect(t.toasts[0]).toMatch(/^Validation step unit is no longer followed: /);
    expect(t.toasts[0]).not.toContain('could not be reached');
    expect(t.waits).toEqual([0, 8_000]);
    expect(t.delays).toEqual([]);
    t.controller.dispose();
  });

  it('keeps following a failed result while the policy allows more attempts, reading the answer’s shape', async () => {
    const t = setup([
      { result: { state: 'failed' }, attempts_left: 2 },
      { result: { state: 'failed' }, attempts_left: 1 },
      { result: { state: 'failed' }, attempts_left: 0 },
    ]);
    await t.start();
    await vi.waitFor(() => {
      expect(t.waits).toEqual([0, 8_000, 8_000]);
    });
    expect(t.toasts).toEqual([]);
    t.controller.dispose();
  });

  it('ends a waiting retry quietly when the view closes, and follows again once reopened', async () => {
    const answers: Answer[] = [running, new TypeError('network')];
    const t = setup(answers, { holdDelays: true });
    await t.start();
    await vi.waitFor(() => {
      expect(t.delays).toEqual([1_000]);
    });
    // Leaving the view starts a new generation: the waiting retry ends without asking or a toast.
    t.viewMode.value = 'list';
    for (const release of t.held.splice(0)) release();
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(t.waits).toEqual([0, 8_000]);
    expect(t.toasts).toEqual([]);
    // Back on the view, a new Run Validation follows to its result again.
    answers.push(running, passed);
    t.viewMode.value = 'orchestration';
    await t.start();
    await vi.waitFor(() => {
      expect(t.waits).toEqual([0, 8_000, 0, 8_000]);
    });
    expect(t.delays).toEqual([1_000]);
    expect(t.toasts).toEqual([]);
    t.controller.dispose();
  });

  it('keeps a permission refusal for its answer, read from the refusal itself', async () => {
    const t = setup([new ApiHttpError('needs permission', 403, 'permission_required', 'req-1')]);
    await t.start();
    expect(t.toasts).toEqual(['A validation step waits for permission.']);
    expect(t.waits).toEqual([0]);
    t.controller.dispose();
  });
});

describe('Use Template… through the view (HS2-A4RVYA)', () => {
  const template = {
    template_id: 'T1',
    name: 'Feature',
    version: 1,
    state: 'active',
    inputs: [
      { key: 'feature', label: 'Feature', kind: 'text', required: true },
      { key: 'flag', label: 'Flag', kind: 'boolean' },
    ],
    goal: { title: '{{feature}}' },
    steps: [{ key: 'api', target: 'T', title: 'Build {{feature}}' }],
  };

  function setup(options: { applyFails?: Error; choice?: boolean } = {}) {
    const project = { id: 'orch', name: 'orch', root: '/synthetic/orch', apiPath: '/api/orch' } as Project;
    const calls: Array<[string, Record<string, unknown>]> = [],
      opened: string[] = [],
      toasts: string[] = [];
    let templates: unknown[] = [template];
    const known: Record<string, unknown> = {
      orchestrationPlanning: () =>
        Promise.resolve({ goals: [{ connection_id: 'c', native_id: 'g1', slug: 'LCH-1', steps: 2 }] }),
      orchestrationTimeline: (_owner: string, after?: string) =>
        after ? new Promise<never>(() => undefined) : Promise.resolve(demoTimeline('proposed')),
      orchestrationTemplates: (_owner: string, operation: string, body: Record<string, unknown>) => {
        calls.push([operation, body]);
        if (operation === 'list') return Promise.resolve({ templates, applies: [] });
        if (operation === 'preview')
          return Promise.resolve({
            template: { template_id: body.template_id },
            problems: (body.inputs as Record<string, unknown>).feature
              ? []
              : [{ code: 'input_missing', at: 'inputs.feature' }],
            goal_title: (body.inputs as Record<string, string | undefined>).feature ?? '',
            steps: [],
            targets: [],
            planning: 'propose',
          });
        if (operation === 'apply') {
          if (options.applyFails) return Promise.reject(options.applyFails);
          return Promise.resolve({
            operation_id: body.operation_id,
            template_id: 'T1',
            snapshot: { name: 'Feature', version: 1 },
            state: 'planning',
            planning: 'propose',
            goal: { connection_id: 'c', native_id: 'g9', slug: 'LCH-9', created: true },
            outcome: 'applied',
          });
        }
        if (operation === 'save_goal') {
          templates = [...templates, { ...template, template_id: 'T2', name: String(body.name) }];
          return Promise.resolve({ ...template, template_id: 'T2', name: body.name });
        }
        return Promise.reject(new Error(operation));
      },
    };
    const client = new Proxy(known, {
      get: (target, key: string) => target[key] ?? (() => new Promise<never>(() => undefined)),
    }) as unknown as Api;
    const controller = createOrchestrationViewController({
      project: () => project,
      viewMode: signal<WorkspaceViewMode>('orchestration'),
      fallbackViewMode: () => 'list',
      orchestrationViewMode: signal<OrchestrationProjectViewMode>('orchestration'),
      mobile: () => false,
      confirm: () => Promise.resolve(false),
      showToast: (message) => void toasts.push(message),
      openTicket: (match) => {
        opened.push(match.qualifiedId);
        return Promise.resolve();
      },
      showPermissionRequest: () => Promise.resolve(),
      openOrchestrationSettings: () => undefined,
      persistWorkspacePreferences: () => undefined,
      planner: {
        availability: () =>
          options.choice === false ? { available: false, reason: 'Choose an AI tool' } : { available: true },
        choice: () => (options.choice === false ? undefined : { tool: 'claude' }),
      } as never,
      personId: () => 'person-1',
      createClient: () => client,
      orchestrationProject: () => true,
    });
    return { controller, calls, opened, toasts };
  }

  it('opens on the first template, previews on a committed change, and applies under one retained id', async () => {
    const t = setup();
    await vi.waitFor(() => {
      expect(t.controller.timeline.value).toBeDefined();
    });
    await t.controller.openTemplates();
    await vi.waitFor(() => {
      expect(t.controller.templateDialog.value?.preview?.template_id).toBe('T1');
    });
    const state = () => t.controller.templateDialog.value!;
    expect(state().selected).toBe('T1');
    expect(state().inputs).toEqual({ feature: '', flag: false });
    expect(state().goals.map((goal) => goal.slug)).toEqual(['LCH-1']);
    expect(state().projects.length).toBeGreaterThan(0);
    // Typing changes the input at once; a pause in typing, or the change event, reads the summary again.
    t.controller.editTemplateInput('feature', 'Sea', false);
    t.controller.editTemplateInput('feature', 'Search', false);
    expect(t.calls.filter(([operation]) => operation === 'preview')).toHaveLength(1);
    expect(state().changed).toEqual(['feature']);
    await vi.waitFor(() => {
      expect(t.calls.filter(([operation]) => operation === 'preview')).toHaveLength(2);
    });
    expect(t.calls.filter(([operation]) => operation === 'preview')[1][1].inputs).toEqual({
      feature: 'Search',
      flag: false,
    });
    // The change after the pause asks nothing new: the same inputs were just read.
    t.controller.editTemplateInput('feature', 'Search', true);
    await vi.waitFor(() => {
      expect(state().preview?.goal_title).toBe('Search');
    });
    expect(state().changed).toEqual([]);
    t.controller.chooseTemplateStep('api', true);
    expect(state().choices).toEqual({ api: 'create' });
    t.controller.chooseTemplateStep('api', false);
    expect(state().choices).toEqual({});
    await t.controller.applyTemplate();
    expect(state().mode).toBe('result');
    expect(state().result?.goal?.slug).toBe('LCH-9');
    const applies = t.calls.filter(([operation]) => operation === 'apply');
    expect(applies[0][1]).toMatchObject({
      template_id: 'T1',
      inputs: { feature: 'Search', flag: false },
      tool: 'claude',
    });
    // Apply Again repeats the same apply; an edited input starts a new one.
    await t.controller.applyTemplate();
    t.controller.selectTemplate('T1');
    t.controller.editTemplateInput('feature', 'Other', false);
    await t.controller.applyTemplate();
    const ids = t.calls.filter(([operation]) => operation === 'apply').map(([, body]) => body.operation_id);
    expect(ids[0]).toBe(ids[1]);
    expect(ids[2]).not.toBe(ids[0]);
    expect(String(ids[0])).toMatch(/^tpl-/);
    await t.controller.openTemplateGoal();
    expect(t.opened).toEqual(['c:g9']);
    expect(t.controller.templateDialog.value).toBeUndefined();
    t.controller.dispose();
  });

  it('says why an apply failed, and that no AI tool is chosen, without applying', async () => {
    const refused = new ApiHttpError('template_apply_conflict', 409, 'template_apply_conflict');
    const t = setup({ applyFails: refused });
    await t.controller.openTemplates();
    await vi.waitFor(() => {
      expect(t.controller.templateDialog.value?.selected).toBe('T1');
    });
    t.controller.editTemplateInput('feature', 'X', false);
    await t.controller.applyTemplate();
    expect(t.controller.templateDialog.value?.failure).toBe(
      'This apply was already made with other inputs. Apply the template again from the start. (template_apply_conflict)',
    );
    expect(t.controller.templateDialog.value?.busy).toBe(false);
    const none = setup({ choice: false });
    await none.controller.openTemplates();
    await vi.waitFor(() => {
      expect(none.controller.templateDialog.value?.selected).toBe('T1');
    });
    await none.controller.applyTemplate();
    expect(none.controller.templateDialog.value?.failure).toBe('Choose an AI tool.');
    expect(none.calls.some(([operation]) => operation === 'apply')).toBe(false);
    t.controller.dispose();
    none.controller.dispose();
  });

  it('saves a goal as a template and selects it; closing and the phone back control reset the dialog', async () => {
    const t = setup();
    await t.controller.openTemplates();
    await vi.waitFor(() => {
      expect(t.controller.templateDialog.value?.selected).toBe('T1');
    });
    t.controller.openSaveTemplate();
    expect(t.controller.templateDialog.value).toMatchObject({ mode: 'save', detailActive: true });
    expect(t.controller.templateDialog.value?.save.goal).toBe('g1');
    t.controller.editSaveTemplate({ name: 'Feature again', targetsAsInputs: true });
    await t.controller.submitSaveTemplate();
    const save = t.calls.find(([operation]) => operation === 'save_goal')!;
    expect(save[1]).toEqual({
      goal: { connection_id: 'c', native_id: 'g1' },
      name: 'Feature again',
      targets_as_inputs: true,
    });
    await vi.waitFor(() => {
      expect(t.controller.templateDialog.value?.selected).toBe('T2');
    });
    expect(t.toasts).toEqual(['Saved the template Feature again.']);
    // Cancel returns to the template; on a phone the back control returns to the list.
    t.controller.openSaveTemplate();
    t.controller.cancelSaveTemplate();
    expect(t.controller.templateDialog.value?.mode).toBe('template');
    t.controller.selectTemplate('T1');
    expect(t.controller.templateDialog.value?.detailActive).toBe(true);
    t.controller.backToTemplates();
    expect(t.controller.templateDialog.value?.detailActive).toBe(false);
    t.controller.closeTemplates();
    expect(t.controller.templateDialog.value).toBeUndefined();
    t.controller.dispose();
  });
});

describe('Triggers… through the view (HS2-KXKA7X)', () => {
  const saved = {
    trigger_id: 'TR1',
    name: 'Night shift',
    source: {
      kind: 'schedule',
      schedule: {
        time_zone: 'UTC',
        local_time: '02:00',
        recurrence: { kind: 'daily', every: 1, start_date: '2026-11-01' },
      },
    },
    action: { placement: 'start' },
    state: 'dry_run',
    updated_at_ms: 1,
    history: [],
    next_runs_ms: [1, 2, 3],
    reviewed: false,
    running: false,
  };

  function setup(options: { confirm?: boolean; overview?: boolean } = {}) {
    const project = { id: 'orch', name: 'Launch', root: '/synthetic/orch', apiPath: '/api/orch' } as Project;
    const calls: Array<[string, Record<string, unknown>]> = [],
      toasts: string[] = [];
    let triggers: Array<Record<string, unknown>> = [saved];
    // HS2-X3FZJE: the overview's other reads answer (refused), so the overview settles with its Triggers row.
    const refused = () => Promise.reject(new Error('refused'));
    const known: Record<string, unknown> = {
      ...(options.overview
        ? { orchestrationDecisions: refused, orchestrationSchedulerPlan: refused, orchestrationSummary: refused }
        : {}),
      orchestrationPlanning: () => Promise.resolve({}),
      orchestrationTimeline: (_owner: string, after?: string) =>
        after ? new Promise<never>(() => undefined) : Promise.resolve(demoTimeline('proposed')),
      orchestrationTemplates: () => Promise.resolve({ templates: [], applies: [] }),
      orchestrationTriggers: (_owner: string, operation: string, body: Record<string, unknown>) => {
        calls.push([operation, body]);
        switch (operation) {
          case 'list':
            return Promise.resolve({ triggers });
          case 'review':
            return Promise.resolve({
              problems: [],
              scope: {},
              permissions: [],
              next: {},
              digest: 'd',
              turn_on: 'person',
            });
          case 'set': {
            const added = { ...saved, ...body, trigger_id: body.trigger_id ?? 'TR2', state: body.start };
            triggers = [...triggers.filter((trigger) => trigger.trigger_id !== added.trigger_id), added];
            return Promise.resolve(added);
          }
          case 'on':
          case 'pause':
          case 'resume':
            triggers = triggers.map((trigger) =>
              trigger.trigger_id === body.trigger_id
                ? { ...trigger, state: operation === 'on' ? 'on' : operation === 'pause' ? 'paused' : 'dry_run' }
                : trigger,
            );
            return Promise.resolve(triggers.find((trigger) => trigger.trigger_id === body.trigger_id));
          default:
            return Promise.reject(new Error(operation));
        }
      },
    };
    const client = new Proxy(known, {
      get: (target, key: string) => target[key] ?? (() => new Promise<never>(() => undefined)),
    }) as unknown as Api;
    const controller = createOrchestrationViewController({
      project: () => project,
      viewMode: signal<WorkspaceViewMode>('orchestration'),
      fallbackViewMode: () => 'list',
      orchestrationViewMode: signal<OrchestrationProjectViewMode>('orchestration'),
      mobile: () => false,
      confirm: () => Promise.resolve(options.confirm ?? true),
      showToast: (message) => void toasts.push(message),
      openTicket: () => Promise.resolve(),
      showPermissionRequest: () => Promise.resolve(),
      openOrchestrationSettings: () => undefined,
      persistWorkspacePreferences: () => undefined,
      planner: { availability: () => ({ available: true }), choice: () => ({ tool: 'claude' }) } as never,
      personId: () => 'person-1',
      createClient: () => client,
      orchestrationProject: () => true,
    });
    const operations = () => calls.map(([operation]) => operation);
    const replace = (next: Array<Record<string, unknown>>) => {
      triggers = next;
    };
    return { controller, calls, toasts, operations, replace };
  }

  it('HS2-X3FZJE reads the triggers with the overview and again on an orchestration change', async () => {
    const t = setup({ overview: true });
    await vi.waitFor(() => {
      expect(t.controller.overview.value?.triggers?.map((trigger) => trigger.trigger_id)).toEqual(['TR1']);
    });
    expect(t.controller.overview.value?.error).toBe('Some of the scheduler’s records could not be read.');
    // Emptied, then refilled: the row follows each change.
    for (const next of [[], [saved, { ...saved, trigger_id: 'TR2', state: 'on' }]]) {
      t.replace(next);
      t.controller.sessionsChanged('orch');
      await vi.waitFor(() => {
        expect(t.controller.overview.value?.triggers).toHaveLength(next.length);
      });
    }
    // The list Triggers… reads is the row's too.
    t.replace([saved]);
    await t.controller.openTriggers();
    expect(t.controller.overview.value?.triggers?.map((trigger) => trigger.trigger_id)).toEqual(['TR1']);
    t.controller.closeTriggers();
    // Another project's change reads nothing.
    const lists = t.operations().filter((operation) => operation === 'list').length;
    t.controller.sessionsChanged('elsewhere');
    expect(t.operations().filter((operation) => operation === 'list')).toHaveLength(lists);
    t.controller.dispose();
  });

  it('opens on the first trigger with its review, sets one up as a dry run, turns it on and pauses it', async () => {
    const t = setup();
    await vi.waitFor(() => {
      expect(t.controller.timeline.value).toBeDefined();
    });
    await t.controller.openTriggers();
    await vi.waitFor(() => {
      expect(t.controller.triggerDialog.value?.review).toBeDefined();
    });
    const opened = t.controller.triggerDialog.value!;
    expect(opened).toMatchObject({ selected: 'TR1', mode: 'detail', ownerName: 'Launch', detailActive: false });
    expect(t.calls.find(([operation]) => operation === 'review')?.[1]).toEqual({ trigger_id: 'TR1' });

    // New Trigger…: a setup defaulting to the planner's tool, reviewed as an unsaved definition.
    t.controller.newTrigger();
    expect(t.controller.triggerDialog.value).toMatchObject({ mode: 'setup', editing: undefined, detailActive: true });
    expect(t.controller.triggerDialog.value?.form.tool).toBe('claude');
    // A committed change reads the review now; the same definition again reads nothing.
    t.controller.editTriggerField('form-name', 'Morning shift', true);
    t.controller.editTriggerField('form-placement', 'start', true);
    await vi.waitFor(() => {
      expect(t.controller.triggerDialog.value?.reviewing).toBe(false);
    });
    const reviews = t.operations().filter((operation) => operation === 'review').length;
    t.controller.editTriggerField('form-name', 'Morning shift', true);
    expect(t.operations().filter((operation) => operation === 'review').length).toBe(reviews);
    const lastReview = t.calls.filter(([operation]) => operation === 'review').at(-1)![1];
    expect(lastReview.trigger).toMatchObject({ name: 'Morning shift', action: { placement: 'start' } });

    await t.controller.saveTrigger();
    const set = t.calls.find(([operation]) => operation === 'set')![1];
    expect(set).toMatchObject({ name: 'Morning shift', start: 'dry_run', action: { placement: 'start' } });
    expect(set.trigger_id).toBeUndefined();
    expect(t.toasts.at(-1)).toBe('Saved the trigger Morning shift as a dry run.');
    expect(t.controller.triggerDialog.value).toMatchObject({ mode: 'detail', selected: 'TR2' });

    // Turn On… is confirmed first; declining sends nothing.
    await t.controller.turnOnTrigger();
    expect(t.calls.find(([operation]) => operation === 'on')?.[1]).toEqual({ trigger_id: 'TR2' });
    expect(t.controller.triggerDialog.value?.triggers.find((trigger) => trigger.trigger_id === 'TR2')?.state).toBe(
      'on',
    );
    await t.controller.pauseTrigger();
    expect(t.controller.triggerDialog.value?.triggers.find((trigger) => trigger.trigger_id === 'TR2')?.state).toBe(
      'paused',
    );
    await t.controller.resumeTrigger();
    expect(t.operations().filter((operation) => operation === 'resume')).toHaveLength(1);

    // Edit…: the saved definition, saved back under its id.
    t.controller.editTrigger();
    expect(t.controller.triggerDialog.value).toMatchObject({ mode: 'setup', editing: 'TR2' });
    expect(t.controller.triggerDialog.value?.form.name).toBe('Morning shift');
    t.controller.cancelTriggerSetup();
    expect(t.controller.triggerDialog.value?.mode).toBe('detail');

    // An orchestration change reads the triggers again while the dialog shows one.
    const lists = t.operations().filter((operation) => operation === 'list').length;
    t.controller.sessionsChanged('orch');
    await vi.waitFor(() => {
      expect(t.operations().filter((operation) => operation === 'list').length).toBe(lists + 1);
    });
    t.controller.closeTriggers();
    expect(t.controller.triggerDialog.value).toBeUndefined();
    t.controller.dispose();
  });

  it('sends nothing when Turn On… is declined', async () => {
    const t = setup({ confirm: false });
    await t.controller.openTriggers();
    await t.controller.turnOnTrigger();
    expect(t.operations()).not.toContain('on');
    t.controller.dispose();
  });

  it('edits each setup field, resetting event inputs when the template changes', () => {
    const t = setup();
    let form = editTriggerForm(t.controller.triggerDialog.value?.form ?? ({} as never), 'nothing', 'x');
    expect(form).toEqual({});
    const base = editTriggerForm(
      {
        name: '',
        when: 'schedule',
        schedule: {
          repeats: 'daily',
          every: '1',
          weekdays: [1],
          startDate: '2026-11-01',
          time: '09:00',
          timeZone: 'UTC',
        },
        target: '',
        event: 'branch_ci_failed',
        tag: '',
        category: '',
        provider: '',
        repository: '',
        workflow: '',
        branch: 'main',
        template: 'T1',
        tool: '',
        eventInputs: { commit: 'commit' },
        placement: '',
        start: 'dry_run',
        overlap: 'skip',
      } as never,
      'form-when',
      'event',
    );
    expect(base.when).toBe('event');
    expect(editTriggerForm(base, 'form-when', 'sometimes').when).toBe('event');
    form = editTriggerForm(base, 'event-input-sha', 'commit');
    expect(form.eventInputs).toEqual({ commit: 'commit', sha: 'commit' });
    expect(editTriggerForm(form, 'event-input-sha', '').eventInputs).toEqual({ commit: 'commit' });
    expect(editTriggerForm(form, 'form-template', 'T1').eventInputs).toEqual(form.eventInputs);
    expect(editTriggerForm(form, 'form-template', 'T2').eventInputs).toEqual({});
    expect(editTriggerForm(form, 'schedule-repeats', 'minutely').schedule).toMatchObject({
      repeats: 'minutely',
      every: '15',
    });
    expect(editTriggerForm(form, 'schedule-repeats', 'hourly').schedule).toMatchObject({
      repeats: 'hourly',
      every: '1',
    });
    // HS2-SEEMXR: every recurrence field the command-schedule dialog has, by its draft key.
    expect(editTriggerForm(form, 'schedule-weekdays', ['5', '2']).schedule.weekdays).toEqual([2, 5]);
    expect(editTriggerForm(form, 'schedule-scheduleType', 'once').schedule.scheduleType).toBe('once');
    expect(editTriggerForm(form, 'schedule-repeats', 'selected_dates').schedule.repeats).toBe('selected_dates');
    expect(editTriggerForm(form, 'schedule-on', 'weekday_ordinal').schedule.on).toBe('weekday_ordinal');
    expect(editTriggerForm(form, 'schedule-endDate', '2027-01-31').schedule.endDate).toBe('2027-01-31');
    expect(editTriggerForm(form, 'schedule-count', '12').schedule.count).toBe('12');
    expect(editTriggerForm(form, 'schedule-excludeDates', '2026-12-25').schedule.excludeDates).toBe('2026-12-25');
    expect(editTriggerForm(form, 'schedule-timeZone', 'Asia/Tokyo').schedule.timeZone).toBe('Asia/Tokyo');
    expect(editTriggerForm(form, 'schedule-paused', true).schedule).toEqual(form.schedule);
    expect(editTriggerForm(form, 'form-placement', 'none').placement).toBe('');
    expect(editTriggerForm(form, 'form-overlap', 'queue_latest').overlap).toBe('queue_latest');
    expect(editTriggerForm(form, 'form-start', 'paused').start).toBe('paused');
    expect(editTriggerForm(form, 'form-event', 'ticket_opened').event).toBe('ticket_opened');
    expect(editTriggerForm(form, 'form-event', 'bogus').event).toBe('branch_ci_failed');
    t.controller.dispose();
  });
});
