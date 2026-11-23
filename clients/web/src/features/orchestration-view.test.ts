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
  orchestrationPersonId,
  parseOrchestrationDetails,
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
    expect(delays).toEqual([1_000, 2_000]);
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
    await expect(planner.propose(project)).rejects.toThrow(/open goal ticket/);
    propose.mockRejectedValueOnce(new ApiHttpError('boom', 500, 'internal'));
    await expect(planner.propose(project)).rejects.toThrow('boom');
    await expect(planner.propose({ ...project, id: 'other' })).rejects.toThrow(/Choose an AI tool/);
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
