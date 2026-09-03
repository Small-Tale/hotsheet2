import { describe, expect, it } from 'vitest';

import type { TicketRow } from './api';
import { hasUnresolvedBlocker } from './ticket-blocking';

const row=(id:string,status:string,blocked_by:string[]=[]):TicketRow=>({connection_id:'git',native_id:id,qualified_id:`git:${id}`,id,slug:`HS2-${id}`,title:id,status,up_next:false,feedback_needed:false,tags:[],blocked_by,claim_count:0});

describe('ticket blocker projection',()=>{
  it('clears Completed and Verified dependencies while retaining unresolved or missing ones',()=>{const completed=row('DONE','completed'),verified=row('CHECK','verified'),waiting=row('WAIT','started');expect(hasUnresolvedBlocker(row('TARGET','started',['DONE','CHECK']),[completed,verified,waiting])).toBe(false);expect(hasUnresolvedBlocker(row('TARGET','started',['DONE','WAIT']),[completed,verified,waiting])).toBe(true);expect(hasUnresolvedBlocker(row('TARGET','started',['MISSING']),[completed,verified,waiting])).toBe(true)});
  it('accepts the slug identity used by non-git providers',()=>{const completed=row('DONE','completed');expect(hasUnresolvedBlocker(row('TARGET','started',[completed.slug]),[completed])).toBe(false)});
});
