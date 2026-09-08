import {describe,expect,it} from 'vitest';

import {addSearchFilter,filterAdvancedSearchResults,searchMatchLabel} from './advanced-search';
import type {TicketRow} from './api';

const row=(slug:string,status='not_started',extra:Partial<TicketRow>={}):TicketRow=>({connection_id:'local',native_id:slug,qualified_id:`local:${slug}`,id:slug,slug,title:slug,status,up_next:false,feedback_needed:false,tags:[],blocked_by:[],claim_count:0,...extra});
describe('advanced search semantics',()=>{
  it('keeps ordinary search view-independent and hides lifecycle tickets except exact slugs',()=>{const rows=[row('HS2-ONE'),row('HS2-HIDDEN','archive')];expect(filterAdvancedSearchResults(rows,'words','working',[])).toEqual([rows[0]]);expect(filterAdvancedSearchResults(rows,'HS2-HIDDEN','working',[])).toEqual(rows)});
  it('supports current scope and explicit filters',()=>{const rows=[row('HS2-ONE'),row('HS2-TWO','backlog',{up_next:true})];expect(filterAdvancedSearchResults(rows,'','current',[],new Set([rows[0].qualified_id]))).toEqual([rows[0]]);expect(filterAdvancedSearchResults(rows,'','working',['status:backlog','up-next'])).toEqual([rows[1]])});
  it('replaces lifecycle filters and clearly labels ticket references',()=>{expect(addSearchFilter(['status:archive'],'status:deleted')).toEqual(['status:deleted']);expect(searchMatchLabel(row('HS2-ONE','not_started',{details:'Depends on HS2-TWO'}),'HS2-TWO')).toBe('Ticket reference')});
});
