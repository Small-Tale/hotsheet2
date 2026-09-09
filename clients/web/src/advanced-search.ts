import type { TicketRow } from './api';

export type SearchScope='working'|'current'|'all';
export type SearchFilter='status:backlog'|'status:archive'|'status:deleted'|'up-next'|'needs-review'|'blocked';

export const searchScopeChoices=[
  {value:'working' as const,label:'Working tickets'},
  {value:'current' as const,label:'Current view'},
  {value:'all' as const,label:'All tickets'},
];
export const searchFilters:ReadonlyArray<{id:SearchFilter;label:string}>=[
  {id:'status:backlog',label:'Backlog'},
  {id:'status:archive',label:'Archived'},
  {id:'status:deleted',label:'Deleted'},
  {id:'up-next',label:'Up Next'},
  {id:'needs-review',label:'Needs review'},
  {id:'blocked',label:'Blocked'},
];
const hiddenStatuses=new Set(['backlog','archive','deleted','moved']);
const normalized=(value:string)=>value.trim().toLowerCase();
export function isExactTicketSlug(ticket:TicketRow,query:string){const value=normalized(query);return [ticket.slug,ticket.native_id,ticket.qualified_id].some(id=>normalized(id)===value)}
export function filterAdvancedSearchResults(rows:TicketRow[],query:string,scope:SearchScope,filters:readonly SearchFilter[],currentIds?:ReadonlySet<string>){
  const lifecycle=filters.find(filter=>filter.startsWith('status:'))?.slice(7);
  return rows.filter(ticket=>{
    if(lifecycle&&ticket.status!==lifecycle)return false;
    if(!lifecycle&&scope==='working'&&hiddenStatuses.has(ticket.status??'')&&!isExactTicketSlug(ticket,query))return false;
    if(scope==='current'&&!currentIds?.has(ticket.qualified_id))return false;
    if(filters.includes('up-next')&&!ticket.up_next)return false;
    if(filters.includes('needs-review')&&!ticket.feedback_needed)return false;
    if(filters.includes('blocked')&&!ticket.blocked_reason?.trim())return false;
    return true;
  });
}
export function addSearchFilter(filters:readonly SearchFilter[],filter:SearchFilter){return filter.startsWith('status:')?[...filters.filter(item=>!item.startsWith('status:')),filter]:filters.includes(filter)?[...filters]:[...filters,filter]}
export function searchMatchLabel(ticket:TicketRow,query:string){const value=normalized(query);if(!value)return 'Ticket';if(isExactTicketSlug(ticket,query))return 'Exact ticket';if(normalized(ticket.title).includes(value))return 'Title';if(ticket.tags.some(tag=>normalized(tag).includes(value)))return 'Tag';if(normalized(ticket.details??'').includes(value))return /^hs\d*-[a-z0-9]+$/i.test(query.trim())?'Ticket reference':'Details';if((ticket.notes??[]).some(note=>normalized(note.text).includes(value)))return /^hs\d*-[a-z0-9]+$/i.test(query.trim())?'Ticket reference':'Note';return 'Indexed content'}
