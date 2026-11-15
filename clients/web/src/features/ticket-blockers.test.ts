import { signal } from 'kerfjs';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { ApiHttpError, type Capabilities, type CheckoutTicketRows, type FullTicket, type TicketRow } from '../api';
import type { Project } from '../interactions/types';
import { duplicateTargetKey } from '../ticket-close';
import { createTicketBlockersController } from './ticket-blockers';

const server = vi.hoisted(() => ({
  tickets: new Map<string, Record<string, unknown>>(),
  reads: [] as string[],
  writes: [] as { id: string; value: Record<string, unknown> }[],
  /** Errors thrown by the next updates, in order. */
  failures: [] as Error[],
  /** A concurrent edit applied to the ticket just before the next update is checked. */
  concurrent: undefined as ((ticket: Record<string, unknown>) => void) | undefined,
  rows: [] as unknown[],
  searches: [] as string[],
}));

type StoredTicket = FullTicket & { store: string };

vi.mock('../api', async (original) => {
  const actual = await original<typeof import('../api')>();
  return {
    ...actual,
    Api: class extends actual.Api {
      override checkoutTicket = async (_checkout: string, id: string) => {
        server.reads.push(id);
        const ticket = server.tickets.get(id);
        if (!ticket) throw new actual.ApiHttpError('not found', 404);
        return { store: 'git-a', ticket: structuredClone(ticket) as unknown as StoredTicket };
      };
      override updateCheckoutTicket = async (_checkout: string, id: string, value: Record<string, unknown>) => {
        server.writes.push({ id, value });
        const failure = server.failures.shift();
        if (failure) throw failure;
        const ticket = server.tickets.get(id)!;
        if (server.concurrent) {
          server.concurrent(ticket);
          server.concurrent = undefined;
        }
        if (value.expected_token !== undefined && value.expected_token !== ticket.concurrency_token)
          throw new actual.ApiHttpError('changed', 409, 'ticket_changed');
        Object.assign(ticket, {
          blocked_by: value.blocked_by,
          concurrency_token: `${String(ticket.concurrency_token)}+`,
        });
        return {
          store: 'git-a',
          ticket: structuredClone(ticket) as unknown as StoredTicket,
          feedbackDelivery: undefined,
        };
      };
      override checkoutTickets = async (_checkout: string, query?: unknown) => {
        server.searches.push((query as { text: string }).text);
        return { items: server.rows, partial: false, truncated: false } as CheckoutTicketRows;
      };
    },
  };
});

const project = { id: 'alpha', name: 'Alpha', apiPath: '/__hotsheet/project-api/alpha' } as Project;
const row = (native: string, slug: string, status = 'not_started', connection = 'git-a') =>
  ({
    id: native,
    slug,
    title: `Synthetic ${slug}`,
    status,
    connection_id: connection,
    native_id: native,
    qualified_id: `${connection}:${native}`,
    blocked_by: [] as string[],
    concurrency_token: 't0',
  }) as unknown as TicketRow & { concurrency_token: string };

function setup(options: { dependencies?: boolean; canUpdate?: boolean; loaded?: TicketRow[] } = {}) {
  const source = row('src', 'HS2-SRC'),
    selectedTicket = signal<FullTicket | null>(structuredClone(source) as unknown as FullTicket),
    tickets = signal<TicketRow[]>(options.loaded ?? [source]),
    showToast = vi.fn();
  server.tickets.set('git-a:src', structuredClone(source) as unknown as Record<string, unknown>);
  const controller = createTicketBlockersController({
    project: () => project,
    selectedTicket,
    tickets,
    capabilitiesFor: () => ({ dependencies: options.dependencies ?? true }) as Capabilities,
    sourceName: () => 'Hot Sheet Git',
    canUpdateSelected: () => options.canUpdate ?? true,
    showToast,
    searchDelayMs: 0,
  });
  return { controller, selectedTicket, tickets, showToast };
}
const settle = async () => {
  await vi.runAllTimersAsync();
};

describe('createTicketBlockersController (HS2-CHH28E)', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    server.tickets.clear();
    server.reads.length = 0;
    server.writes.length = 0;
    server.failures.length = 0;
    server.concurrent = undefined;
    server.searches.length = 0;
    server.rows = [row('src', 'HS2-SRC'), row('one', 'HS2-ONE'), row('gh', 'GH-1', 'not_started', 'gh-a')];
    server.tickets.set('git-a:one', row('one', 'HS2-ONE') as unknown as Record<string, unknown>);
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it('opens, searches the inspected project, selects, and adds a blocker with the concurrency token', async () => {
    const { controller, selectedTicket, tickets, showToast } = setup();
    expect(controller.open()).toBe(true);
    expect(controller.dialog.value?.source.slug).toBe('HS2-SRC');
    controller.search('synthetic');
    expect(controller.dialog.value?.searching).toBe(true);
    await settle();
    expect(server.searches).toEqual(['synthetic']);
    const one = controller.dialog.value?.candidates.find((candidate) => candidate.slug === 'HS2-ONE');
    controller.select(duplicateTargetKey(one!));
    expect(controller.dialog.value?.selected?.slug).toBe('HS2-ONE');
    await controller.submit();
    expect(server.writes).toEqual([{ id: 'git-a:src', value: { blocked_by: ['one'], expected_token: 't0' } }]);
    expect(controller.dialog.value).toBeUndefined();
    expect(selectedTicket.value?.blocked_by).toEqual(['one']);
    expect(tickets.value[0]?.blocked_by).toEqual(['one']);
    expect(showToast).toHaveBeenCalledWith('HS2-ONE now blocks HS2-SRC.');
  });

  it('keeps a server refusal such as a dependency cycle in the dialog, then succeeds on retry', async () => {
    const { controller, showToast } = setup();
    controller.open();
    controller.search('one');
    await settle();
    controller.select(duplicateTargetKey(controller.dialog.value!.candidates.find((c) => c.slug === 'HS2-ONE')!));
    server.failures.push(new ApiHttpError('HS2-ONE already depends on HS2-SRC (dependency cycle)', 400));
    await controller.submit();
    expect(controller.dialog.value?.error).toContain('dependency cycle');
    expect(controller.dialog.value?.submitting).toBe(false);
    expect(showToast).not.toHaveBeenCalled();
    await controller.submit();
    expect(controller.dialog.value).toBeUndefined();
  });

  it('rereads and retries once after a concurrent edit, keeping the other client’s blocker', async () => {
    const { controller, selectedTicket } = setup();
    controller.open();
    controller.search('one');
    await settle();
    controller.select(duplicateTargetKey(controller.dialog.value!.candidates.find((c) => c.slug === 'HS2-ONE')!));
    server.concurrent = (ticket) => {
      Object.assign(ticket, { blocked_by: ['other'], concurrency_token: 't1' });
    };
    await controller.submit();
    expect(server.writes.map((write) => write.value)).toEqual([
      { blocked_by: ['one'], expected_token: 't0' },
      { blocked_by: ['other', 'one'], expected_token: 't1' },
    ]);
    expect(selectedTicket.value?.blocked_by).toEqual(['other', 'one']);
  });

  it('refuses invalid choices locally and does not open where blockers cannot be edited', async () => {
    const { controller } = setup();
    controller.open();
    await controller.submit();
    expect(controller.dialog.value?.error).toBe('Select the ticket that blocks HS2-SRC.');
    expect(server.writes).toEqual([]);
    controller.search('gh');
    await settle();
    // The other source's ticket and the ticket itself are never offered.
    expect(controller.dialog.value?.candidates.map((candidate) => candidate.slug)).not.toContain('GH-1');
    controller.close();
    expect(controller.dialog.value).toBeUndefined();
    expect(setup({ dependencies: false }).controller.open()).toBe(false);
    expect(setup({ canUpdate: false }).controller.open()).toBe(false);
  });

  it('projects rows from loaded tickets, resolves the rest once, and marks missing ones unavailable', async () => {
    const done = row('done', 'HS2-DONE', 'completed');
    server.tickets.set('git-a:done', done as unknown as Record<string, unknown>);
    const { controller, selectedTicket } = setup({ loaded: [row('src', 'HS2-SRC'), row('one', 'HS2-ONE')] });
    selectedTicket.value = { ...selectedTicket.value!, blocked_by: ['one', 'done', 'gone'] };
    await settle();
    expect(server.reads.sort()).toEqual(['git-a:done', 'git-a:gone']);
    const props = controller.sectionProps()!;
    expect(props.rows.map((item) => [item.slug ?? item.nativeId, item.state])).toEqual([
      ['HS2-ONE', 'open'],
      ['HS2-DONE', 'done'],
      ['gone', 'unavailable'],
    ]);
    expect(props.availability).toEqual({ enabled: true });
    // A repeated selection does not refetch what is already resolved.
    selectedTicket.value = { ...selectedTicket.value };
    await settle();
    expect(server.reads).toHaveLength(2);
    controller.dispose();
    expect(setup({ dependencies: false }).controller.sectionProps()?.availability).toEqual({
      enabled: false,
      disabledReason: 'Hot Sheet Git tickets cannot record blocking dependencies.',
    });
  });

  it('removes one blocker, shows the row busy meanwhile, and reports a failure in a toast', async () => {
    const { controller, selectedTicket, showToast } = setup();
    server.tickets.get('git-a:src')!.blocked_by = ['one', 'two'];
    selectedTicket.value = { ...selectedTicket.value!, blocked_by: ['one', 'two'] };
    await settle();
    const pending = controller.remove('one');
    expect(controller.sectionProps()?.removing).toEqual(['one']);
    await pending;
    expect(server.writes.at(-1)?.value).toEqual({ blocked_by: ['two'], expected_token: 't0' });
    expect(selectedTicket.value.blocked_by).toEqual(['two']);
    expect(showToast).toHaveBeenLastCalledWith('HS2-ONE no longer blocks HS2-SRC.');
    expect(controller.sectionProps()?.removing).toEqual([]);
    server.failures.push(new Error('offline'));
    await controller.remove('two');
    expect(showToast).toHaveBeenLastCalledWith('Could not remove two: offline');
    expect(selectedTicket.value.blocked_by).toEqual(['two']);
  });
});
