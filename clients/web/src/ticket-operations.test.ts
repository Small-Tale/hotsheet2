import { describe, expect, it } from 'vitest';

import {
  deduplicateTitle,
  MAX_TICKET_HISTORY_TRANSACTIONS,
  TicketHistory,
  type TicketSnapshot,
} from './ticket-operations';

describe('ticket operation history', () => {
  it('waits for in-flight edits before undo and redo instead of dropping them (HS2-K90QHF)', async () => {
    const ticket: TicketSnapshot = { slug: 'HS2-ONE', status: 'started' };
    const releases: Array<() => void> = [];
    const applied: unknown[] = [];
    const history = new TicketHistory(
      () => ticket,
      (_slug, patch) => {
        Object.assign(ticket, patch);
        applied.push(patch.status);
        return new Promise<boolean>((resolve) => {
          releases.push(() => {
            resolve(true);
          });
        });
      },
    );
    const edit = history.execute(ticket.slug, { status: 'completed' });
    const undo = history.undo();
    const redo = history.redo();
    await Promise.resolve();
    expect(applied).toEqual(['completed']);
    releases.shift()!();
    expect(await edit).toBe(true);
    await expect.poll(() => releases.length).toBe(1);
    expect(applied).toEqual(['completed', 'started']);
    releases.shift()!();
    expect(await undo).toBe(true);
    await expect.poll(() => releases.length).toBe(1);
    releases.shift()!();
    expect(await redo).toBe(true);
    expect(applied).toEqual(['completed', 'started', 'completed']);
    expect(ticket.status).toBe('completed');
    expect(await history.redo()).toBe(false);
  });
  it('keeps undo usable after an in-flight edit fails', async () => {
    const ticket: TicketSnapshot = { slug: 'HS2-ONE', count: 0 };
    let fail = true;
    const history = new TicketHistory(
      () => ticket,
      async (_slug, patch) => {
        if (fail) throw new Error('network');
        Object.assign(ticket, patch);
        return true;
      },
    );
    await expect(history.execute(ticket.slug, { count: 1 })).rejects.toThrow('network');
    expect(await history.undo()).toBe(false);
    fail = false;
    await history.execute(ticket.slug, { count: 2 });
    expect(await history.undo()).toBe(true);
    expect(ticket.count).toBe(0);
  });
  it('retains only recent undo operations across repeated edits and redo', async () => {
    const ticket: TicketSnapshot = { slug: 'HS2-ONE', count: 0 };
    const history = new TicketHistory(
      () => ticket,
      async (_slug, patch) => {
        Object.assign(ticket, patch);
        return true;
      },
    );
    for (let count = 1; count <= MAX_TICKET_HISTORY_TRANSACTIONS + 5; count += 1)
      await history.execute(ticket.slug, { count });
    for (let count = 0; count < MAX_TICKET_HISTORY_TRANSACTIONS; count += 1) expect(await history.undo()).toBe(true);
    expect(ticket.count).toBe(5);
    expect(await history.undo()).toBe(false);
    for (let count = 0; count < MAX_TICKET_HISTORY_TRANSACTIONS; count += 1) expect(await history.redo()).toBe(true);
    expect(ticket.count).toBe(MAX_TICKET_HISTORY_TRANSACTIONS + 5);
    expect(await history.redo()).toBe(false);
  });
  it('walks mixed operations through repeated undo and redo in order', async () => {
    const ticket: TicketSnapshot = { slug: 'HS2-ONE', status: 'not_started', up_next: false };
    const history = new TicketHistory(
      () => ticket,
      async (_slug, patch) => {
        Object.assign(ticket, patch);
        return true;
      },
    );
    await history.execute(ticket.slug, { status: 'started' });
    await history.execute(ticket.slug, { up_next: true });
    await history.undo();
    await history.undo();
    expect(ticket).toMatchObject({ status: 'not_started', up_next: false });
    await history.redo();
    await history.redo();
    expect(ticket).toMatchObject({ status: 'started', up_next: true });
  });
  it('preserves remotely interleaved fields and clears redo after a new branch', async () => {
    const ticket: TicketSnapshot = { slug: 'HS2-ONE', status: 'not_started', priority: 'default' };
    const history = new TicketHistory(
      () => ticket,
      async (_slug, patch) => {
        Object.assign(ticket, patch);
        return true;
      },
    );
    await history.execute(ticket.slug, { status: 'started', priority: 'high' });
    ticket.priority = 'urgent';
    await history.undo();
    expect(ticket).toMatchObject({ status: 'not_started', priority: 'urgent' });
    await history.execute(ticket.slug, { status: 'backlog' });
    expect(await history.redo()).toBe(false);
  });
  it('uses the HS1-compatible copy suffix case-insensitively', () => {
    expect(deduplicateTitle('Fix it', ['fix it', 'Fix it (Copy)', 'Fix it (Copy 2)'])).toBe('Fix it (Copy 3)');
  });
  it('treats a multi-ticket drop as one undo transaction', async () => {
    const tickets = new Map<string, TicketSnapshot>([
      ['A', { slug: 'A', status: 'started' }],
      ['B', { slug: 'B', status: 'started' }],
    ]);
    const history = new TicketHistory(
      (slug) => tickets.get(slug),
      async (slug, patch) => {
        Object.assign(tickets.get(slug)!, patch);
        return true;
      },
    );
    await history.executeMany([
      { slug: 'A', patch: { status: 'backlog' } },
      { slug: 'B', patch: { status: 'backlog' } },
    ]);
    await history.undo();
    expect(tickets.get('A')?.status).toBe('started');
    expect(tickets.get('B')?.status).toBe('started');
  });
  it('undoes and redoes an already-applied external transaction', async () => {
    const calls: string[] = [];
    const history = new TicketHistory(
      () => undefined,
      async () => false,
    );
    history.recordExternal(
      async () => {
        calls.push('undo');
        return true;
      },
      async () => {
        calls.push('redo');
        return true;
      },
    );
    expect(await history.undo()).toBe(true);
    expect(await history.redo()).toBe(true);
    expect(calls).toEqual(['undo', 'redo']);
  });
  it('keeps a failed external transaction available to retry', async () => {
    let attempts = 0;
    const history = new TicketHistory(
      () => undefined,
      async () => false,
    );
    history.recordExternal(
      async () => {
        attempts += 1;
        return false;
      },
      async () => true,
    );
    expect(await history.undo()).toBe(false);
    expect(await history.undo()).toBe(false);
    expect(attempts).toBe(2);
  });
});
