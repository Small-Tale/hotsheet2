import {isTicketActivelyWorkedOn} from './active-ticket-work';
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
type SearchExpression={kind:'term';value:string}|{kind:'not';value:SearchExpression}|{kind:'and'|'or';left:SearchExpression;right:SearchExpression};
type Lexeme={kind:'term'|'and'|'or'|'not'|'left'|'right';value:string};
const lifecycleAliases:Partial<Record<string,string[]>>= {
  'not-started':['not_started'],started:['started'],completed:['completed'],verified:['verified'],backlog:['backlog'],backlogged:['backlog'],archived:['archive'],open:['not_started','started'],closed:['completed','verified','archive'],
};

function lexSearchExpression(query:string):Lexeme[]{
  const result:Lexeme[]=[];let index=0;
  while(index<query.length){
    if(/\s/.test(query[index])){index+=1;continue}
    if(query[index]==='('){result.push({kind:'left',value:'('});index+=1;continue}
    if(query[index]===')'){result.push({kind:'right',value:')'});index+=1;continue}
    if(query[index]==='"'){
      let value='',closed=false;index+=1;
      while(index<query.length){const character=query[index++];if(character==='\\'&&index<query.length){value+=query[index++];continue}if(character==='"'){closed=true;break}value+=character}
      result.push({kind:'term',value:closed?value:`"${value}`});continue;
    }
    const start=index;while(index<query.length&&!/[\s()]/.test(query[index]))index+=1;
    const value=query.slice(start,index),keyword=value.toLowerCase();result.push({kind:keyword==='and'||keyword==='or'||keyword==='not'?keyword:'term',value});
  }
  return result;
}

function parseSearchExpression(query:string):SearchExpression|undefined{
  const lexemes=lexSearchExpression(query);if(!lexemes.length)return undefined;let cursor=0;
  const primary=():SearchExpression|undefined=>{const token=lexemes.at(cursor);if(!token)return undefined;if(token.kind==='term'){cursor+=1;return{kind:'term',value:token.value}}if(token.kind==='left'){cursor+=1;const value=or();if(!value||lexemes.at(cursor)?.kind!=='right')return undefined;cursor+=1;return value}return undefined};
  const not=():SearchExpression|undefined=>{if(lexemes[cursor]?.kind==='not'){cursor+=1;const value=not();return value?{kind:'not',value}:undefined}return primary()};
  const and=():SearchExpression|undefined=>{let left=not();if(!left)return undefined;while(cursor<lexemes.length&&lexemes[cursor].kind!=='right'&&lexemes[cursor].kind!=='or'){if(lexemes[cursor].kind==='and')cursor+=1;const right=not();if(!right)return undefined;left={kind:'and',left,right}}return left};
  const or=():SearchExpression|undefined=>{let left=and();if(!left)return undefined;while(lexemes[cursor]?.kind==='or'){cursor+=1;const right=and();if(!right)return undefined;left={kind:'or',left,right}}return left};
  const expression=or();return expression&&cursor===lexemes.length?expression:undefined;
}

function ticketContains(ticket:TicketRow,value:string){const needle=normalized(value);return Boolean(needle)&&[ticket.slug,ticket.native_id,ticket.qualified_id,ticket.title,ticket.details??'',...ticket.tags,...(ticket.notes??[]).map(note=>note.text)].some(field=>normalized(field).includes(needle))}
function matchesTerm(ticket:TicketRow,value:string,now:number){const match=value.match(/^is:(.+)$/i),state=match?.[1].toLowerCase();if(!state)return ticketContains(ticket,value);if(state==='up-next')return ticket.up_next;if(state==='active')return isTicketActivelyWorkedOn(ticket,now);if(state==='duplicate')return ticket.close_reason==='duplicate';const statuses=lifecycleAliases[state];return statuses ? statuses.includes(ticket.status??'') : false}
function evaluateSearchExpression(ticket:TicketRow,expression:SearchExpression,now:number):boolean{switch(expression.kind){case'term':return matchesTerm(ticket,expression.value,now);case'not':return!evaluateSearchExpression(ticket,expression.value,now);case'and':return evaluateSearchExpression(ticket,expression.left,now)&&evaluateSearchExpression(ticket,expression.right,now);case'or':return evaluateSearchExpression(ticket,expression.left,now)||evaluateSearchExpression(ticket,expression.right,now)}}
export function usesAdvancedSearchExpression(query:string){return /(?:^|[\s(])(?:AND|OR|NOT)(?=$|[\s)])|[()]|(?:^|\s)is:(?:up-next|active|open|closed|duplicate|not-started|started|completed|verified|backlog|backlogged|archived)(?=$|\s|\))/i.test(query)}
export function matchesSearchExpression(ticket:TicketRow,query:string,now=Date.now()){const expression=parseSearchExpression(query);return expression?evaluateSearchExpression(ticket,expression,now):ticketContains(ticket,query)}
export function isExactTicketSlug(ticket:TicketRow,query:string){const value=normalized(query);return [ticket.slug,ticket.native_id,ticket.qualified_id].some(id=>normalized(id)===value)}
export function filterAdvancedSearchResults(rows:TicketRow[],query:string,scope:SearchScope,filters:readonly SearchFilter[],currentIds?:ReadonlySet<string>){
  const lifecycle=filters.find(filter=>filter.startsWith('status:'))?.slice(7),advanced=usesAdvancedSearchExpression(query);
  return rows.filter(ticket=>{
    if(lifecycle&&ticket.status!==lifecycle)return false;
    if(!lifecycle&&!advanced&&scope==='working'&&hiddenStatuses.has(ticket.status??'')&&!isExactTicketSlug(ticket,query))return false;
    if(scope==='current'&&!currentIds?.has(ticket.qualified_id))return false;
    if(filters.includes('up-next')&&!ticket.up_next)return false;
    if(filters.includes('needs-review')&&!ticket.feedback_needed)return false;
    if(filters.includes('blocked')&&!ticket.blocked_reason?.trim())return false;
    if(advanced&&!matchesSearchExpression(ticket,query))return false;
    return true;
  });
}
export function addSearchFilter(filters:readonly SearchFilter[],filter:SearchFilter){return filter.startsWith('status:')?[...filters.filter(item=>!item.startsWith('status:')),filter]:filters.includes(filter)?[...filters]:[...filters,filter]}
export function searchMatchLabel(ticket:TicketRow,query:string){const value=normalized(query);if(!value)return 'Ticket';if(isExactTicketSlug(ticket,query))return 'Exact ticket';if(normalized(ticket.title).includes(value))return 'Title';if(ticket.tags.some(tag=>normalized(tag).includes(value)))return 'Tag';if(normalized(ticket.details??'').includes(value))return /^hs\d*-[a-z0-9]+$/i.test(query.trim())?'Ticket reference':'Details';if((ticket.notes??[]).some(note=>normalized(note.text).includes(value)))return /^hs\d*-[a-z0-9]+$/i.test(query.trim())?'Ticket reference':'Note';return 'Indexed content'}
