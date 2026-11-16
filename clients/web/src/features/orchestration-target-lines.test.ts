import { signal } from 'kerfjs';
import { describe, expect, it } from 'vitest';

import type { Project } from '../interactions/types';
import type { OrchestrationTargetLines } from '../orchestration-timeline';
import { findTimelineItem } from '../orchestration-timeline-board';
import { demoTimeline } from '../ux-demo/orchestration-fixtures';
import { createOrchestrationTargetLines, mergeTargetLines, targetLinesClientFor } from './orchestration-target-lines';

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

describe('the lines of a remote project’s tab (HS2-3D8D6V, HS2-SHBSQF)', () => {
  const binding = '01ARZ3NDEKTSV4RRFFQ69G5FAV';
  const remote = { id: `remote:${binding}:api-0123456789ab`, name: 'peer · API', apiPath: '/api/remote' } as Project;
  const other = { ...waiting, target: { ...waiting.target, native_id: 'other-ticket' }, step_id: 'step-other' };
  const answer = (
    version: string,
    owner: string,
    items = [waiting],
    orchestrated = true,
  ): OrchestrationTargetLines => ({
    version,
    orchestrated,
    lines: items.map((item) => ({ owner_checkout_id: owner, owner_label: owner, proposed: false, item })),
  });

  /** Both servers: answers queue per half; a read with nothing queued stays open until aborted. */
  function halves(here: Array<OrchestrationTargetLines | Error>, there: Array<OrchestrationTargetLines | Error>) {
    const asked: string[] = [];
    const aborted: string[] = [];
    const next = (queue: Array<OrchestrationTargetLines | Error>, label: string, abort?: AbortSignal) => {
      const value = queue.shift();
      if (value === undefined)
        return new Promise<OrchestrationTargetLines>((_, reject) => {
          abort?.addEventListener('abort', () => {
            aborted.push(label);
            reject(new DOMException('aborted', 'AbortError'));
          });
        });
      return value instanceof Error ? Promise.reject(value) : Promise.resolve(value);
    };
    const api = (apiPath: string) => ({
      orchestrationTargetLines: (_checkout: string, after?: string, abort?: AbortSignal) => {
        asked.push(`there ${after ?? '-'}`);
        expect(apiPath).toBe('/api/remote');
        return next(there, `there ${after ?? '-'}`, abort);
      },
      orchestrationRemoteTargetLines: (registration: string, checkout: string, after?: string, abort?: AbortSignal) => {
        asked.push(`here ${after ?? '-'}`);
        expect([apiPath, registration, checkout]).toEqual(['/api/owner', binding, 'api-0123456789ab']);
        return next(here, `here ${after ?? '-'}`, abort);
      },
    });
    return { asked, aborted, api };
  }

  it('merges this machine’s lines with the pushed ones, this machine winning per ticket', () => {
    const merged = mergeTargetLines(answer('h1', 'mine'), answer('t1', 'pushed', [waiting, other]));
    expect(merged.version).toBe('h1~t1');
    expect(merged.orchestrated).toBe(true);
    expect(merged.lines.map((line) => [line.owner_checkout_id, line.item.target.native_id])).toEqual([
      ['mine', waiting.target.native_id],
      ['pushed', 'other-ticket'],
    ]);
    expect(mergeTargetLines(undefined, answer('t', 'x', [], false))).toMatchObject({
      version: '-~t',
      orchestrated: false,
      lines: [],
    });
    expect(mergeTargetLines(answer('h', 'x', [], false), answer('t', 'pushed', [other])).orchestrated).toBe(true);
  });

  it('reads both halves first, then waits on both cursors and answers the half that changes', async () => {
    const fake = halves([answer('h1', 'mine')], [answer('t1', 'pushed', [other]), answer('t2', 'pushed', [])]);
    const client = targetLinesClientFor(remote, [remote, project('owner')], fake.api);
    const first = await client.orchestrationTargetLines(remote.id);
    expect(first.version).toBe('h1~t1');
    expect(first.lines).toHaveLength(2);
    // The combined cursor splits: each half waits on its own version; the remote half changes first and
    // the waiting read of this machine is cancelled.
    const changed = await client.orchestrationTargetLines(remote.id, 'h1~t1');
    expect(changed.version).toBe('h1~t2');
    expect(changed.lines.map((line) => line.owner_checkout_id)).toEqual(['mine']);
    expect(fake.asked).toEqual(['here -', 'there -', 'here h1', 'there t1']);
    expect(fake.aborted).toEqual(['here h1']);
  });

  it('keeps waiting on one half while the other fails, and fails only when both do', async () => {
    const fake = halves(
      [answer('h1', 'mine'), answer('h2', 'mine', [])],
      [new Error('remote server unavailable'), new Error('still unavailable')],
    );
    const client = targetLinesClientFor(remote, [remote, project('owner')], fake.api);
    // A failed half reads as unread; the answer is this machine’s alone.
    const first = await client.orchestrationTargetLines(remote.id);
    expect(first.version).toBe('h1~-');
    // The unread half is asked again with no cursor; its failure leaves the other half waiting.
    const changed = await client.orchestrationTargetLines(remote.id, 'h1~-');
    expect(changed.version).toBe('h2~-');
    expect(fake.asked).toEqual(['here -', 'there -', 'here h1', 'there -']);
    const down = halves([new Error('down')], [new Error('down too')]);
    await expect(
      targetLinesClientFor(remote, [remote, project('owner')], down.api).orchestrationTargetLines(remote.id),
    ).rejects.toThrow('down');
  });

  it('answers a stale cursor at once with what it knows, and a caller’s abort cancels both waits', async () => {
    const fake = halves([answer('h1', 'mine')], [answer('t1', 'pushed')]);
    const client = targetLinesClientFor(remote, [remote, project('owner')], fake.api);
    await client.orchestrationTargetLines(remote.id);
    expect((await client.orchestrationTargetLines(remote.id, 'old~old')).version).toBe('h1~t1');
    expect(fake.asked).toEqual(['here -', 'there -']);
    const controller = new AbortController();
    const waiting = client.orchestrationTargetLines(remote.id, 'h1~t1', controller.signal);
    await settle();
    controller.abort();
    await expect(waiting).rejects.toThrow();
    expect(fake.aborted.sort()).toEqual(['here h1', 'there t1']);
  });

  it('keeps a local project on its own server, and a remote tab with no local project on the remote one', async () => {
    const asked: string[] = [];
    const api = (apiPath: string) => ({
      orchestrationTargetLines: (checkout: string) => {
        asked.push(`${apiPath} ${checkout}`);
        return Promise.resolve(lines('v1'));
      },
      orchestrationRemoteTargetLines: () => Promise.reject(new Error('not asked')),
    });
    await targetLinesClientFor(project('api'), [project('api')], api).orchestrationTargetLines('api');
    await targetLinesClientFor(remote, [remote], api).orchestrationTargetLines(remote.id);
    expect(asked).toEqual(['/api/api api', `/api/remote ${remote.id}`]);
  });

  it('shows both halves on the remote tab’s tickets and long polls the combined version', async () => {
    const fake = halves(
      [answer('h1', 'mine')],
      [answer('t1', 'pushed', [other]), answer('t2', 'pushed', [{ ...other, goal_slug: 'OTHER-2' }])],
    );
    const shown = signal<Project | undefined>(remote);
    const controller = createOrchestrationTargetLines({
      shown,
      client: (current) => targetLinesClientFor(current, [remote, project('owner')], fake.api),
      afterPaint: now,
    });
    await settle();
    await settle();
    expect(controller.lineFor(remote.id, ticket)?.goal).toBe(waiting.goal_slug);
    expect(
      controller.lineFor(remote.id, { connection_id: other.target.connection_id, native_id: 'other-ticket' })?.goal,
    ).toBe('OTHER-2');
    // The long poll now waits on h1 and t2 together.
    expect(fake.asked.slice(-2)).toEqual(['here h1', 'there t2']);
    // The first wait was ended by the remote change; leaving the ticket views ends the second, both halves.
    expect(fake.aborted).toEqual(['here h1']);
    shown.value = undefined;
    await settle();
    expect(fake.aborted.slice(1).sort()).toEqual(['here h1', 'there t2']);
    controller.dispose();
  });
});
