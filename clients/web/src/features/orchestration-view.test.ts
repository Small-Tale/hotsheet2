import { describe, expect, it, vi } from 'vitest';

import { ApiHttpError } from '../api';
import type { Project } from '../interactions/types';
import type { OrchestrationTimeline } from '../orchestration-timeline';
import { demoTimeline } from '../ux-demo/orchestration-fixtures';
import {
  coalescedPlacement,
  createServerPlanner,
  orchestrationPersonId,
  parseOrchestrationDetails,
  planningPhase,
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
    propose.mockRejectedValueOnce(new ApiHttpError('no goals', 409, 'plan_no_goals'));
    await expect(planner.propose(project)).rejects.toThrow(/open goal ticket/);
    propose.mockRejectedValueOnce(new ApiHttpError('boom', 500, 'internal'));
    await expect(planner.propose(project)).rejects.toThrow('boom');
    await expect(planner.propose({ ...project, id: 'other' })).rejects.toThrow(/Choose an AI tool/);
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
