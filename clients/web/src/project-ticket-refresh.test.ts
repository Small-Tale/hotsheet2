import { describe, expect, it, vi } from 'vitest';

import { appendUniqueTicketRows, loadProjectTicketRefresh } from './project-ticket-refresh';

describe('loadProjectTicketRefresh', () => {
  const counts = {total:1,queued:1,backlog:0,archive:0,open:1,up_next:0,active:0,started:0,completed_today:0};
  it('accepts the legacy ticket-array response while compatible servers transition to pagination',async()=>{const ticket={id:'1',slug:'HS2-LEGACY',title:'Legacy',tags:[]};const result=await loadProjectTicketRefresh({checkoutTicketPage:vi.fn().mockResolvedValue([ticket]),checkoutCorruptTickets:vi.fn().mockResolvedValue([])},'checkout');expect(result).toEqual({tickets:[ticket],corruptTickets:[]})});
  it('keeps healthy tickets when the corrupt-ticket index fails', async () => {
    const ticket = { id: '01', slug: 'HS2-OK', title: 'Healthy', tags: [] };
    const checkoutTicketPage=vi.fn().mockResolvedValue({items:[ticket],counts});
    const result = await loadProjectTicketRefresh({
      checkoutTicketPage,
      checkoutCorruptTickets: vi.fn().mockRejectedValue(new Error('index unavailable')),
    }, 'checkout');

    expect(result).toEqual({ tickets: [ticket], ticketCounts:counts, nextCursor:undefined, corruptTicketsError: 'index unavailable' });
    expect(checkoutTicketPage).toHaveBeenCalledWith('checkout',200,undefined,{collection:'queue'});
  });

  it('keeps corrupt entries available when the healthy-ticket index fails', async () => {
    const corrupt = { store: 'local', store_path: '/tickets', path: '/tickets/01.md', slug: 'HS2-BAD', error: 'invalid notes' };
    const result = await loadProjectTicketRefresh({
      checkoutTicketPage: vi.fn().mockRejectedValue(new Error('healthy index unavailable')),
      checkoutCorruptTickets: vi.fn().mockResolvedValue([corrupt]),
    }, 'checkout');

    expect(result).toEqual({ ticketsError: 'healthy index unavailable', corruptTickets: [corrupt] });
  });

  it('removes a stale healthy row when live diagnostics identify the same slug as corrupt', async () => {
    const healthy = { id: '01', slug: 'HS2-OK', title: 'Healthy', tags: [] };
    const stale = { id: '02', slug: 'HS2-BAD', title: 'Stale indexed title', tags: [] };
    const corrupt = { store: 'local', store_path: '/tickets', path: '/tickets/02.md', slug: 'HS2-BAD', error: 'invalid notes' };
    const result = await loadProjectTicketRefresh({
      checkoutTicketPage: vi.fn().mockResolvedValue({items:[healthy,stale],counts,next_cursor:'next'}),
      checkoutCorruptTickets: vi.fn().mockResolvedValue([corrupt]),
    }, 'checkout');

    expect(result).toEqual({ tickets: [healthy], ticketCounts:counts, nextCursor:'next', corruptTickets: [corrupt] });
  });
});

describe('appendUniqueTicketRows',()=>{
  const ticket=(connection_id:string,native_id:string)=>({connection_id,native_id,qualified_id:`${connection_id}:${native_id}`,id:native_id,slug:`HS2-${native_id}`,title:native_id,up_next:false,feedback_needed:false,tags:[],blocked_by:[],claim_count:0});
  it('appends a continuation page without duplicating overlapping or repeated rows',()=>{
    const first=ticket('local','1'),second=ticket('local','2');
    expect(appendUniqueTicketRows([first],[first,second,second])).toEqual([first,second]);
  });
});
