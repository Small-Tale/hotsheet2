import { describe, expect, it } from 'vitest';

import { deduplicateTitle, TicketHistory, type TicketSnapshot } from './ticket-operations';

describe('ticket operation history', () => {
  it('walks mixed operations through repeated undo and redo in order', async () => {
    const ticket: TicketSnapshot = { slug: 'HS2-ONE', status: 'not_started', up_next: false };
    const history = new TicketHistory(() => ticket, async (_slug, patch) => { Object.assign(ticket, patch); return true; });
    await history.execute(ticket.slug, { status: 'started' }); await history.execute(ticket.slug, { up_next: true });
    await history.undo(); await history.undo(); expect(ticket).toMatchObject({ status: 'not_started', up_next: false });
    await history.redo(); await history.redo(); expect(ticket).toMatchObject({ status: 'started', up_next: true });
  });
  it('preserves remotely interleaved fields and clears redo after a new branch', async () => {
    const ticket: TicketSnapshot = { slug: 'HS2-ONE', status: 'not_started', priority: 'default' };
    const history = new TicketHistory(() => ticket, async (_slug, patch) => { Object.assign(ticket, patch); return true; });
    await history.execute(ticket.slug, { status: 'started', priority: 'high' }); ticket.priority = 'urgent'; await history.undo();
    expect(ticket).toMatchObject({ status: 'not_started', priority: 'urgent' });
    await history.execute(ticket.slug, { status: 'backlog' }); expect(await history.redo()).toBe(false);
  });
  it('uses the HS1-compatible copy suffix case-insensitively', () => {
    expect(deduplicateTitle('Fix it', ['fix it', 'Fix it (Copy)', 'Fix it (Copy 2)'])).toBe('Fix it (Copy 3)');
  });
  it('treats a multi-ticket drop as one undo transaction', async () => {
    const tickets = new Map<string,TicketSnapshot>([['A',{slug:'A',status:'started'}],['B',{slug:'B',status:'started'}]]);
    const history = new TicketHistory(slug=>tickets.get(slug),async(slug,patch)=>{Object.assign(tickets.get(slug)!,patch);return true;});
    await history.executeMany([{slug:'A',patch:{status:'backlog'}},{slug:'B',patch:{status:'backlog'}}]);
    await history.undo(); expect(tickets.get('A')?.status).toBe('started'); expect(tickets.get('B')?.status).toBe('started');
  });
});
