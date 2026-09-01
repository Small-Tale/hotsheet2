import { describe, expect, it } from 'vitest';

import type { TicketRow } from './api';
import { projectTicketPatch, ticketRowFromFull } from './ticket-mutation';

const row: TicketRow = { connection_id:'git',native_id:'1',qualified_id:'git:1',id:'1',slug:'HS2-1',title:'Before',status:'not_started',up_next:false,feedback_needed:false,tags:[],blocked_by:[],claim_count:0 };

describe('ticket mutation projection', () => {
  it('projects renderable fields immediately without leaking command-only fields', () => {
    expect(projectTicketPatch(row, { up_next:true, note:'not a row field' })).toEqual({ ...row, up_next:true });
  });

  it('reconciles the authoritative response without losing compact-row fields', () => {
    const full = { ...row, title:'After', details:'Body', notes:[], attachments:[] };
    expect(ticketRowFromFull(row, full)).toMatchObject({ slug:'HS2-1', title:'After', details:'Body' });
  });
});
