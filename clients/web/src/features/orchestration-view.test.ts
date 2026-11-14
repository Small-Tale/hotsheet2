import { describe, expect, it, vi } from 'vitest';

import { ApiHttpError } from '../api';
import type { Project } from '../interactions/types';
import type { OrchestrationTimeline } from '../orchestration-timeline';
import { demoTimeline } from '../ux-demo/orchestration-fixtures';
import { createServerPlanner, orchestrationPersonId, startOrchestrationTimelineStream } from './orchestration-view';

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
