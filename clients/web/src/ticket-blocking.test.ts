import { describe, expect, it } from 'vitest';

import type { TicketRow } from './api';
import { hasUnresolvedBlocker } from './ticket-blocking';

const row=(id:string,status:string,blocked_by:string[]=[],blocked_reason?:string):TicketRow=>({connection_id:'git',native_id:id,qualified_id:`git:${id}`,id,slug:`HS2-${id}`,title:id,status,up_next:false,feedback_needed:false,tags:[],blocked_by,blocked_reason,claim_count:0});

describe('ticket blocker projection',()=>{
  it('uses a non-empty blocked reason as the single visible source of truth',()=>{expect(hasUnresolvedBlocker(row('TARGET','started',['WAIT']))).toBe(false);expect(hasUnresolvedBlocker(row('TARGET','started',[],'Waiting for review'))).toBe(true);expect(hasUnresolvedBlocker(row('TARGET','started',[],'   '))).toBe(false)});
});
