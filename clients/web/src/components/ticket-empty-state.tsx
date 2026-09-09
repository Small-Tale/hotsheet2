import './ticket-empty-state.css';

import { Inbox, ListX, Search, SearchX } from 'lucide';

import { LoadingSpinner } from './loading-spinner';
import { LucideIcon } from './lucide-icon';

export type TicketEmptyStateKind='loading'|'project'|'view'|'searching'|'search';
export interface TicketEmptyStateProps {kind:TicketEmptyStateKind;viewLabel?:string;query?:string}

export function ticketEmptyStateCopy({kind,viewLabel='this view',query=''}:TicketEmptyStateProps){
  if(kind==='loading')return{title:'Loading tickets',detail:'Opening this project…',icon:undefined,iconName:'loading'};
  if(kind==='project')return{title:'No tickets yet',detail:'Create a ticket to start planning this project.',icon:Inbox,iconName:'inbox'};
  if(kind==='searching')return{title:'Searching tickets',detail:query?`Looking for “${query}”…`:'Looking for matching tickets…',icon:Search,iconName:'search'};
  if(kind==='search')return{title:query?`No tickets match “${query}”`:'No matching tickets',detail:'Try a different search.',icon:SearchX,iconName:'search-x'};
  return{title:`No tickets in ${viewLabel}`,detail:'Tickets will appear here when they enter this view.',icon:ListX,iconName:'list-x'};
}

/** Shared empty feedback for ticket lists, boards, and individual board columns. */
export function TicketEmptyState(props:TicketEmptyStateProps){const copy=ticketEmptyStateCopy(props);return <div class="ticket-empty-state" data-component="ticket-empty-state" data-kind={props.kind} role="status">{copy.icon?<LucideIcon icon={copy.icon} name={copy.iconName}/>:<LoadingSpinner label="Loading tickets"/>}<strong>{copy.title}</strong><span>{copy.detail}</span></div>}
