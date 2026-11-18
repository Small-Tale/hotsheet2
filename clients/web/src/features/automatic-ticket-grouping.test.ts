import { signal } from 'kerfjs';
import { describe, expect, it, vi } from 'vitest';

import type { ClientTurnEvent, TicketRow, ToolConnection } from '../api';
import type { Project } from '../interactions/types';
import { AUTOMATIC_GROUP_CONNECTION_PREFIX, createAutomaticTicketGrouping } from './automatic-ticket-grouping';

const PROJECT = { id: 'demo-checkout', apiPath: '/api' } as Project;
const GROUPS = [{ id: '7K3M9P2QXA', name: 'Board polish' }];
const ROWS = [
  { slug: 'HS2-AAAAAA', title: 'Fix drag', category: 'bug' },
  { slug: 'HS2-BBBBBB', title: 'Persist widths', category: 'feature' },
] as TicketRow[];

/**
 * A fake drive client that answers each turn through `claimTurnEvent`, the way the change stream
 * delivers the server's turn events. `reply` decides the events for one turn.
 */
function harness(
  options: {
    reply?: (prompt: string) => ClientTurnEvent[] | undefined;
    tool?: string | undefined;
    actions?: ToolConnection['actions'];
    project?: () => Project | undefined;
    assignExisting?: () => Promise<boolean>;
    assignNew?: (name: string) => Promise<{ id: string; name: string }>;
    disabledReason?: string;
    timeoutMs?: number;
    rows?: TicketRow[];
  } = {},
) {
  let ids = 0;
  const calls: string[] = [],
    toasts: string[] = [],
    busy = signal<string | undefined>(undefined),
    busyLabels: (string | undefined)[] = [],
    assignExisting = vi.fn(options.assignExisting ?? (() => Promise.resolve(true))),
    assignNew = vi.fn(options.assignNew ?? ((name: string) => Promise.resolve({ id: 'NEWGROUP01', name }))),
    prompts: string[] = [];
  busy.subscribe((value) => busyLabels.push(value));
  const grouping = createAutomaticTicketGrouping({
    project: options.project ?? (() => PROJECT),
    driveClient: () => ({
      createToolConnection: (value) => {
        calls.push(`create ${value.tool} ${value.connection_id ?? ''} ${value.model ?? ''}`);
        return Promise.resolve({
          id: value.connection_id!,
          tool: value.tool,
          project: '/work',
          role: 'main',
          busy: false,
          actions: options.actions ?? ['send_turn', 'interrupt'],
        } as ToolConnection);
      },
      sendToolTurn: (id, content, _session, _selection, turnId) => {
        calls.push(`turn ${id}`);
        prompts.push(content);
        const events = options.reply?.(content);
        queueMicrotask(() => {
          // A stale turn's event is claimed but ignored.
          grouping.claimTurnEvent({ connection_id: id, turn_id: 'stale', event: { type: 'output', content: 'x' } });
          for (const event of events ?? []) grouping.claimTurnEvent({ connection_id: id, turn_id: turnId, event });
        });
        return Promise.resolve({ id } as ToolConnection);
      },
      interruptToolTurn: (id) => {
        calls.push(`interrupt ${id}`);
        return Promise.resolve({ id } as ToolConnection);
      },
      deleteToolConnection: (_checkout, id) => {
        calls.push(`delete ${id}`);
        return Promise.resolve(undefined);
      },
    }),
    selection: () =>
      ('tool' in options ? options.tool : 'claude') ? { tool: options.tool ?? 'claude', model: 'opus' } : undefined,
    toolLabel: (tool) => (tool === 'claude' ? 'Claude' : tool),
    groups: () => GROUPS,
    rowsFor: (slugs) => (options.rows ?? ROWS).filter((row) => slugs.includes(row.slug)),
    disabledReason: () => options.disabledReason,
    assignExisting,
    assignNew,
    showToast: (message) => toasts.push(message),
    busy,
    timeoutMs: options.timeoutMs ?? 1_000,
    newId: () => `id${++ids}`,
  });
  return { grouping, calls, toasts, busy, busyLabels, assignExisting, assignNew, prompts };
}

/** A per-ticket reply (HS2-4H6XJ1): each slug's `group_id` or `new_group`. */
const assign = (...items: Array<{ slug: string; group_id?: string; new_group?: string }>) =>
  JSON.stringify({ assignments: items });
const answer = (content: string): ClientTurnEvent[] => [
  { type: 'output', content: content.slice(0, 5), truncated: false },
  { type: 'output', content: content.slice(5), truncated: false },
  { type: 'done', reason: 'completed' },
];
const SLUGS = ['HS2-AAAAAA', 'HS2-BBBBBB'];

describe('createAutomaticTicketGrouping (HS2-4C32A0)', () => {
  it('asks the default tool in a private connection and applies an existing group', async () => {
    const { grouping, calls, toasts, busyLabels, assignExisting, prompts } = harness({
      reply: () =>
        answer(assign({ slug: 'HS2-AAAAAA', group_id: '7K3M9P2QXA' }, { slug: 'HS2-BBBBBB', group_id: '7K3M9P2QXA' })),
    });
    expect(await grouping.run(SLUGS)).toBe(true);
    const connection = `${AUTOMATIC_GROUP_CONNECTION_PREFIX}id1`;
    expect(calls).toEqual([`create claude ${connection} opus`, `turn ${connection}`, `delete ${connection}`]);
    expect(prompts[0]).toContain('"slug":"HS2-AAAAAA","title":"Fix drag","category":"bug"');
    expect(assignExisting).toHaveBeenCalledWith(SLUGS, '7K3M9P2QXA');
    expect(toasts).toEqual(['Added 2 tickets to “Board polish”.']);
    expect(busyLabels).toEqual([undefined, 'Asking Claude to choose groups…', undefined]);
  });

  it('creates a proposed new group and moves the tickets into it', async () => {
    const { grouping, toasts, assignNew, assignExisting } = harness({
      reply: () => answer(assign({ slug: 'HS2-AAAAAA', new_group: 'Release prep' })),
    });
    expect(await grouping.run(['HS2-AAAAAA'])).toBe(true);
    expect(assignNew).toHaveBeenCalledWith('Release prep', ['HS2-AAAAAA']);
    expect(assignExisting).not.toHaveBeenCalled();
    expect(toasts).toEqual(['Added 1 ticket to new “Release prep”.']);
  });

  it('groups each ticket on its own and says which fit no group (HS2-4H6XJ1)', async () => {
    const rows = [...ROWS, { slug: 'HS2-CCCCCC', title: 'Changelog', category: 'task' }] as TicketRow[],
      { grouping, toasts, assignExisting, assignNew, prompts } = harness({
        rows,
        reply: () =>
          answer(
            assign(
              { slug: 'HS2-AAAAAA', group_id: '7K3M9P2QXA' },
              { slug: 'HS2-BBBBBB', new_group: 'Release prep' },
              { slug: 'HS2-ZZZZZZ', group_id: '7K3M9P2QXA' },
            ),
          ),
      });
    expect(await grouping.run(['HS2-AAAAAA', 'HS2-BBBBBB', 'HS2-CCCCCC'])).toBe(true);
    expect(prompts).toHaveLength(1);
    expect(prompts[0]).toContain('judging each ticket on its own');
    expect(assignExisting).toHaveBeenCalledTimes(1);
    expect(assignExisting).toHaveBeenCalledWith(['HS2-AAAAAA'], '7K3M9P2QXA');
    expect(assignNew).toHaveBeenCalledWith('Release prep', ['HS2-BBBBBB']);
    expect(toasts).toEqual([
      'Added 1 ticket to “Board polish”, 1 ticket to new “Release prep”. 1 ticket fit no group.',
    ]);
  });

  it('writes nothing when no ticket fits a group (HS2-4H6XJ1)', async () => {
    const { grouping, toasts, assignExisting, assignNew } = harness({ reply: () => answer(assign()) });
    expect(await grouping.run(SLUGS)).toBe(false);
    expect(assignExisting).not.toHaveBeenCalled();
    expect(assignNew).not.toHaveBeenCalled();
    expect(toasts).toEqual(['Claude found no fitting group for 2 tickets; no group was changed.']);
  });

  it('asks about a large selection in chunks and shares a new name across them (HS2-4H6XJ1)', async () => {
    const rows = Array.from({ length: 45 }, (_, index) => ({
        slug: `HS2-T${String(index).padStart(5, '0')}`,
        title: `Ticket ${index}`,
        category: 'task',
      })) as TicketRow[],
      { grouping, prompts, assignNew, toasts } = harness({
        rows,
        reply: (prompt) => {
          const context = JSON.parse(prompt.slice(prompt.indexOf('\n\n') + 2)) as { tickets: { slug: string }[] };
          return answer(assign(...context.tickets.map((ticket) => ({ slug: ticket.slug, new_group: 'Batch' }))));
        },
      });
    expect(await grouping.run(rows.map((row) => row.slug))).toBe(true);
    expect(prompts).toHaveLength(2);
    expect(assignNew).toHaveBeenCalledTimes(1);
    expect((assignNew.mock.calls[0] as unknown as [string, string[]])[1]).toHaveLength(45);
    expect(toasts).toEqual(['Added 45 tickets to new “Batch”.']);
  });

  it('changes nothing and says so when no AI tool is configured', async () => {
    const { grouping, calls, toasts, busyLabels } = harness({ tool: undefined });
    expect(await grouping.run(SLUGS)).toBe(false);
    expect(calls).toEqual([]);
    expect(toasts).toEqual(['Automatic grouping needs an AI tool. Set one up in AI settings; no group was changed.']);
    expect(busyLabels).toEqual([undefined]);
  });

  it('reports a failed turn with its message and changes nothing', async () => {
    const { grouping, calls, toasts, assignExisting, assignNew } = harness({
      reply: () => [{ type: 'done', reason: 'failed', message: 'Selected model is at capacity.' }],
    });
    expect(await grouping.run(SLUGS)).toBe(false);
    expect(toasts).toEqual(['Automatic grouping failed: Selected model is at capacity. No group was changed.']);
    expect(assignExisting).not.toHaveBeenCalled();
    expect(assignNew).not.toHaveBeenCalled();
    expect(calls.at(-1)).toBe(`delete ${AUTOMATIC_GROUP_CONNECTION_PREFIX}id1`);
  });

  it('reports an interrupted turn without a message', async () => {
    const { grouping, toasts } = harness({ reply: () => [{ type: 'done', reason: 'interrupted' }] });
    expect(await grouping.run(SLUGS)).toBe(false);
    expect(toasts).toEqual(['Automatic grouping failed: Claude stopped before answering. No group was changed.']);
  });

  it('interrupts and closes a turn that does not answer in time', async () => {
    vi.useFakeTimers();
    try {
      const { grouping, calls, toasts, busy } = harness({ reply: () => undefined, timeoutMs: 5_000 }),
        running = grouping.run(SLUGS);
      await vi.advanceTimersByTimeAsync(4_999);
      expect(busy.value).toBe('Asking Claude to choose groups…');
      await vi.advanceTimersByTimeAsync(1);
      expect(await running).toBe(false);
      const connection = `${AUTOMATIC_GROUP_CONNECTION_PREFIX}id1`;
      expect(calls.slice(-2)).toEqual([`interrupt ${connection}`, `delete ${connection}`]);
      expect(toasts).toEqual(['Automatic grouping failed: Claude did not answer in time. No group was changed.']);
      expect(busy.value).toBeUndefined();
    } finally {
      vi.useRealTimers();
    }
  });

  it('reports an unusable reply and changes nothing', async () => {
    const { grouping, toasts, assignExisting } = harness({ reply: () => answer('Put them in Board polish.') });
    expect(await grouping.run(SLUGS)).toBe(false);
    expect(toasts).toEqual([
      'Automatic grouping could not use Claude’s answer: The AI’s answer was not the expected JSON. No group was changed.',
    ]);
    expect(assignExisting).not.toHaveBeenCalled();
  });

  it('reports a connection that cannot take a turn, and still closes it', async () => {
    const { grouping, calls, toasts } = harness({ actions: ['interrupt'] });
    expect(await grouping.run(SLUGS)).toBe(false);
    expect(toasts).toEqual([
      'Automatic grouping failed: Claude cannot answer a one-shot request. No group was changed.',
    ]);
    expect(calls).toEqual([
      `create claude ${AUTOMATIC_GROUP_CONNECTION_PREFIX}id1 opus`,
      `delete ${AUTOMATIC_GROUP_CONNECTION_PREFIX}id1`,
    ]);
  });

  it('reports a failed create of the proposed group', async () => {
    const { grouping, toasts } = harness({
      reply: () => answer(assign({ slug: 'HS2-AAAAAA', new_group: 'Release prep' })),
      assignNew: () => Promise.reject(new Error('group_registry_required')),
    });
    expect(await grouping.run(SLUGS)).toBe(false);
    expect(toasts).toEqual(['Automatic grouping could not create “Release prep”: group_registry_required']);
  });

  it('stays silent when the ordinary group write already reported its failure', async () => {
    const { grouping, toasts } = harness({
      reply: () => answer(assign({ slug: 'HS2-AAAAAA', group_id: '7K3M9P2QXA' })),
      assignExisting: () => Promise.resolve(false),
    });
    expect(await grouping.run(SLUGS)).toBe(false);
    expect(toasts).toEqual([]);
  });

  it('refuses ungroupable tickets, an empty selection, no project, and a second run', async () => {
    const blocked = harness({ disabledReason: 'One or more selected ticket providers do not support groups.' });
    expect(await blocked.grouping.run(SLUGS)).toBe(false);
    expect(blocked.toasts).toEqual(['One or more selected ticket providers do not support groups.']);
    const idle = harness({ project: () => undefined });
    expect(await idle.grouping.run(SLUGS)).toBe(false);
    expect(await idle.grouping.run([])).toBe(false);
    const busy = harness({ reply: () => answer(assign({ slug: 'HS2-AAAAAA', group_id: '7K3M9P2QXA' })) }),
      first = busy.grouping.run(SLUGS);
    expect(await busy.grouping.run(SLUGS)).toBe(false);
    expect(await first).toBe(true);
    expect(busy.calls.filter((call) => call.startsWith('create'))).toHaveLength(1);
  });

  it('never writes into a project that replaced the one it asked about', async () => {
    let current: Project | undefined = PROJECT;
    const { grouping, assignExisting } = harness({
      project: () => current,
      reply: () => {
        current = { ...PROJECT, id: 'other' };
        return answer(assign({ slug: 'HS2-AAAAAA', group_id: '7K3M9P2QXA' }));
      },
    });
    expect(await grouping.run(SLUGS)).toBe(false);
    expect(assignExisting).not.toHaveBeenCalled();
  });

  it('claims only its own connections’ events', () => {
    const { grouping } = harness();
    expect(
      grouping.claimTurnEvent({ connection_id: 'hotsheet-sidebar', event: { type: 'done', reason: 'completed' } }),
    ).toBe(false);
    expect(
      grouping.claimTurnEvent({
        connection_id: `${AUTOMATIC_GROUP_CONNECTION_PREFIX}gone`,
        event: { type: 'done', reason: 'completed' },
      }),
    ).toBe(true);
  });
});
