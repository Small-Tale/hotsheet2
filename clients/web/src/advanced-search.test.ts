import {describe,expect,it} from 'vitest';

import {addSearchFilter,filterAdvancedSearchResults,matchesSearchExpression,searchMatchLabel,usesAdvancedSearchExpression} from './advanced-search';
import type {TicketRow} from './api';

const row=(slug:string,status='not_started',extra:Partial<TicketRow>={}):TicketRow=>({connection_id:'local',native_id:slug,qualified_id:`local:${slug}`,id:slug,slug,title:slug,status,up_next:false,feedback_needed:false,tags:[],blocked_by:[],claim_count:0,...extra});
describe('advanced search semantics',()=>{
  it('keeps ordinary search view-independent and hides lifecycle tickets except exact slugs',()=>{const rows=[row('HS2-ONE'),row('HS2-HIDDEN','archive')];expect(filterAdvancedSearchResults(rows,'words','working',[])).toEqual([rows[0]]);expect(filterAdvancedSearchResults(rows,'HS2-HIDDEN','working',[])).toEqual(rows)});
  it('supports current scope and explicit filters',()=>{const rows=[row('HS2-ONE'),row('HS2-TWO','backlog',{up_next:true})];expect(filterAdvancedSearchResults(rows,'','current',[],new Set([rows[0].qualified_id]))).toEqual([rows[0]]);expect(filterAdvancedSearchResults(rows,'','working',['status:backlog','up-next'])).toEqual([rows[1]])});
  it('filters blocked tickets by their visible reason rather than a dependency edge',()=>{const reason=row('HS2-REASON','started',{blocked_reason:'Waiting'}),edge=row('HS2-EDGE','started',{blocked_by:['missing']});expect(filterAdvancedSearchResults([reason,edge],'','all',['blocked'])).toEqual([reason])});
  it('replaces lifecycle filters and clearly labels ticket references',()=>{expect(addSearchFilter(['status:archive'],'status:deleted')).toEqual(['status:deleted']);expect(searchMatchLabel(row('HS2-ONE','not_started',{details:'Depends on HS2-TWO'}),'HS2-TWO')).toBe('Ticket reference')});
  it('supports every is: lifecycle alias and live Up Next/active state',()=>{
    const now=Date.parse('2026-09-09T10:00:00Z'),active=row('HS2-ACTIVE','started',{up_next:true,claimed_by:'worker',claim_lease_expires_at:'2026-09-09T10:05:00Z'}),rows=[row('HS2-NEW'),active,row('HS2-DONE','completed'),row('HS2-VERIFIED','verified'),row('HS2-BACKLOG','backlog'),row('HS2-ARCHIVE','archive'),row('HS2-DUPLICATE','completed',{close_reason:'duplicate',duplicate_of:'git:2'})];
    expect(rows.filter(ticket=>matchesSearchExpression(ticket,'is:up-next',now))).toEqual([active]);
    expect(rows.filter(ticket=>matchesSearchExpression(ticket,'is:active',now))).toEqual([active]);
    expect(rows.filter(ticket=>matchesSearchExpression(ticket,'is:open',now))).toEqual(rows.slice(0,2));
    expect(rows.filter(ticket=>matchesSearchExpression(ticket,'is:closed',now))).toEqual([rows[2],rows[3],rows[5],rows[6]]);
    expect(rows.filter(ticket=>matchesSearchExpression(ticket,'is:duplicate',now))).toEqual([rows[6]]);
    expect(rows.filter(ticket=>matchesSearchExpression(ticket,'is:not-started',now))).toEqual([rows[0]]);
    expect(rows.filter(ticket=>matchesSearchExpression(ticket,'is:backlogged',now))).toEqual([rows[4]]);
    expect(rows.filter(ticket=>matchesSearchExpression(ticket,'is:archived',now))).toEqual([rows[5]]);
    expect(filterAdvancedSearchResults(rows,'is:archived','working',[])).toEqual([rows[5]]);
  });
  it('parses boolean precedence, negation, parentheses, phrases, and implicit AND',()=>{
    const rows=[row('HS2-ONE','started',{title:'Parser repair',tags:['client']}),row('HS2-TWO','completed',{title:'Parser docs',tags:['docs']}),row('HS2-THREE','backlog',{title:'Unrelated',tags:['client']})];
    expect(rows.filter(ticket=>matchesSearchExpression(ticket,'parser AND NOT is:completed'))).toEqual([rows[0]]);
    expect(rows.filter(ticket=>matchesSearchExpression(ticket,'(tag-does-not-exist OR client) AND is:open'))).toEqual([rows[0]]);
    expect(rows.filter(ticket=>matchesSearchExpression(ticket,'"Parser docs" OR is:backlog'))).toEqual([rows[1],rows[2]]);
    expect(rows.filter(ticket=>matchesSearchExpression(ticket,'parser is:completed'))).toEqual([rows[1]]);
    expect(rows.filter(ticket=>matchesSearchExpression(ticket,'NOT tag:client AND parser'))).toEqual([rows[1]]);
    expect(usesAdvancedSearchExpression('ordinary words')).toBe(false);expect(usesAdvancedSearchExpression('NOT (is:archived OR is:backlog)')).toBe(true);
  });
  it('safely rejects incomplete groups and unknown lifecycle aliases',()=>{
    const ticket=row('HS2-ONE','started',{title:'Parser repair'});
    expect(matchesSearchExpression(ticket,'(parser')).toBe(false);
    expect(matchesSearchExpression(ticket,'is:unknown')).toBe(false);
  });
});
