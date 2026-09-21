import { EmptyState } from '@kerfjs/ui/empty-state';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { CircleAlert, Inbox, ListX, Search, SearchX } from 'lucide';

export type TicketEmptyStateKind =
  'loading' | 'view-loading' | 'view-error' | 'project' | 'view' | 'searching' | 'search';
export interface TicketEmptyStateProps {
  kind: TicketEmptyStateKind;
  viewLabel?: string;
  query?: string;
}

export function ticketEmptyStateCopy({ kind, viewLabel = 'this view', query = '' }: TicketEmptyStateProps) {
  if (kind === 'loading')
    return { title: 'Loading tickets', detail: 'Opening this project…', icon: undefined, iconName: 'loading' };
  if (kind === 'view-loading')
    return {
      title: `Loading ${viewLabel}`,
      detail: 'Fetching tickets for this view…',
      icon: undefined,
      iconName: 'loading',
    };
  if (kind === 'view-error')
    return {
      title: `Couldn’t load ${viewLabel}`,
      detail: 'Try selecting this view again.',
      icon: CircleAlert,
      iconName: 'circle-alert',
    };
  if (kind === 'project')
    return {
      title: 'No tickets yet',
      detail: 'Create a ticket to start planning this project.',
      icon: Inbox,
      iconName: 'inbox',
    };
  if (kind === 'searching')
    return {
      title: 'Searching tickets',
      detail: query ? `Looking for “${query}”…` : 'Looking for matching tickets…',
      icon: Search,
      iconName: 'search',
    };
  if (kind === 'search')
    return {
      title: query ? `No tickets match “${query}”` : 'No matching tickets',
      detail: 'Try a different search.',
      icon: SearchX,
      iconName: 'search-x',
    };
  return {
    title: `No tickets in ${viewLabel}`,
    detail: 'Tickets will appear here when they enter this view.',
    icon: ListX,
    iconName: 'list-x',
  };
}

/** Shared empty feedback for ticket lists, boards, and individual board columns. */
export function TicketEmptyState(props: TicketEmptyStateProps) {
  const copy = ticketEmptyStateCopy(props),
    busy = props.kind === 'loading' || props.kind === 'view-loading';
  return (
    <EmptyState
      className={`ticket-empty-state ticket-empty-state--${props.kind}`}
      title={copy.title}
      detail={copy.detail}
      busy={busy}
      icon={copy.icon ? <LucideIcon icon={copy.icon} name={copy.iconName} /> : undefined}
    />
  );
}
