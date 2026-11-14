import { signal } from 'kerfjs';
import { describe, expect, it } from 'vitest';

import type { Project } from '../interactions/types';
import type { OrchestrationTargetLines } from '../orchestration-timeline';
import { findTimelineItem } from '../orchestration-timeline-board';
import { demoTimeline } from '../ux-demo/orchestration-fixtures';
import { createOrchestrationTargetLines } from './orchestration-target-lines';

const project = (id: string) => ({ id, name: id, apiPath: `/api/${id}` }) as Project;
const waiting = findTimelineItem(demoTimeline('running'), 'step-6')!.item;
const lines = (version: string, orchestrated = true, state = waiting.state): OrchestrationTargetLines => ({
  version,
  orchestrated,
  lines: orchestrated
    ? [{ owner_checkout_id: 'launch', owner_label: 'Launch', proposed: false, item: { ...waiting, state } }]
    : [],
});
const ticket = { connection_id: waiting.target.connection_id, native_id: waiting.target.native_id };
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));
const now = (run: () => void) => {
  run();
  return () => undefined;
};

/** A server double: answers queue in order; a read with nothing queued stays open until stopped. */
function server(answers: Array<OrchestrationTargetLines | Error>) {
  const asked: Array<{ checkout: string; after?: string }> = [];
  const open: AbortSignal[] = [];
  return {
    asked,
    open,
    client: () => ({
      orchestrationTargetLines: (checkout: string, after?: string, abort?: AbortSignal) => {
        asked.push({ checkout, after });
        const next = answers.shift();
        if (next === undefined) {
          if (abort) open.push(abort);
          return new Promise<OrchestrationTargetLines>(() => undefined);
        }
        return next instanceof Error ? Promise.reject(next) : Promise.resolve(next);
      },
    }),
  };
}

describe('a target project’s orchestration lines (HS2-S3FKJ7)', () => {
  it('reads once and holds no request open for a project nothing orchestrates', async () => {
    const fake = server([lines('v0', false)]);
    const shown = signal<Project | undefined>(project('api'));
    const controller = createOrchestrationTargetLines({ shown, client: fake.client, afterPaint: now });
    await settle();
    expect(fake.asked).toEqual([{ checkout: 'api', after: undefined }]);
    expect(controller.lineFor('api', ticket)).toBeUndefined();
    controller.dispose();
  });

  it('long polls an orchestrated project by version and shows each change on the ticket it names', async () => {
    const fake = server([lines('v1'), lines('v2', true, 'running')]);
    const shown = signal<Project | undefined>(project('api'));
    const controller = createOrchestrationTargetLines({ shown, client: fake.client, afterPaint: now });
    await settle();
    await settle();
    expect(fake.asked.map((read) => read.after)).toEqual([undefined, 'v1', 'v2']);
    // The running state speaks through the status badge, so the line keeps its place and goal only.
    expect(controller.lineFor('api', ticket)).toMatchObject({ state: undefined, goal: waiting.goal_slug });
    expect(controller.lineFor('other', ticket)).toBeUndefined();
    expect(controller.lineFor('api', { ...ticket, native_id: 'nope' })).toBeUndefined();
    // Leaving the ticket views stops the open read; the lines last read stay for the next cached paint.
    shown.value = undefined;
    expect(fake.open.at(-1)?.aborted).toBe(true);
    expect(controller.lineFor('api', ticket)).toMatchObject({ goal: waiting.goal_slug });
    controller.dispose();
  });

  it('switches projects, keeps lines through a failure, and refills after empty', async () => {
    const fake = server([
      lines('a1'),
      new Error('offline'),
      lines('a2'),
      { version: 'a3', orchestrated: true, lines: [] },
      lines('a4'),
    ]);
    const delays: number[] = [];
    const shown = signal<Project | undefined>(project('api'));
    const controller = createOrchestrationTargetLines({
      shown,
      client: fake.client,
      afterPaint: now,
      delay: (ms) => {
        delays.push(ms);
        return Promise.resolve();
      },
    });
    for (let turn = 0; turn < 8; turn += 1) await settle();
    expect(delays).toEqual([1_000]);
    expect(fake.asked.map((read) => read.after)).toEqual([undefined, 'a1', 'a1', 'a2', 'a3', 'a4']);
    expect(controller.lineFor('api', ticket)?.state).toBe('Waits for permission');
    shown.value = project('web');
    expect(fake.asked.at(-1)).toEqual({ checkout: 'web', after: undefined });
    // The other project's lines stay cached for switching back; they never show for this one.
    expect(controller.lineFor('api', ticket)?.state).toBe('Waits for permission');
    expect(controller.lineFor('web', ticket)).toBeUndefined();
    controller.dispose();
    expect(fake.open.every((abort) => abort.aborted)).toBe(true);
  });

  it('reads only after the paint, and a switch before then reads nothing for the old project (HS2-8YHY6V)', async () => {
    const fake = server([lines('b1', false)]);
    const scheduled: Array<{ run: () => void; cancelled: boolean }> = [];
    const shown = signal<Project | undefined>(project('api'));
    const controller = createOrchestrationTargetLines({
      shown,
      client: fake.client,
      afterPaint: (run) => {
        const entry = { run, cancelled: false };
        scheduled.push(entry);
        return () => {
          entry.cancelled = true;
        };
      },
    });
    expect(fake.asked).toEqual([]);
    shown.value = project('web');
    expect(scheduled.map((entry) => entry.cancelled)).toEqual([true, false]);
    expect(fake.asked).toEqual([]);
    scheduled[1].run();
    await settle();
    expect(fake.asked).toEqual([{ checkout: 'web', after: undefined }]);
    controller.dispose();
  });
});
