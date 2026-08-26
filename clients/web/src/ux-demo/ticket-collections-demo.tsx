import { signal } from 'kerfjs';
import { TicketList } from '../components/ticket-list';
import { TicketBoard, type TicketColumnProps } from '../components/ticket-board';
import type { TicketRowProps } from '../components/ticket-row';

const initialTickets: TicketRowProps[] = [
  { slug: 'HS2-R76MMW', title: 'Build TicketList and TicketBoard around shared responsive TicketRow', status: 'started', priority: 'high', category: 'feature', tags: ['client', 'ux'], upNext: true, busy: true, categoryIcon: 'sparkles', categoryColor: '#3b82f6', agentName: 'Codex', updatedLabel: 'Now' },
  { slug: 'HS2-K00QPZ', title: 'Expose StatusBadge plain appearance as a supported demo variant', status: 'completed', priority: 'default', category: 'bug', tags: ['client'], needsReview: true, categoryIcon: 'bug', categoryColor: '#ef4444', agentName: 'Claude', updatedLabel: '8m ago' },
  { slug: 'HS2-RPVFA4', title: 'Add a repository-status snapshot and server endpoint', status: 'verified', priority: 'default', category: 'task', tags: ['core', 'server'], categoryColor: '#14b8a6', agentName: 'AI', updatedLabel: '2h ago' },
  { slug: 'HS2-JN3X4W', title: 'Define safe command execution, streaming, cancellation, and history contracts', status: 'started', priority: 'urgent', category: 'feature', tags: ['server', 'commands', 'security'], blocked: true, categoryIcon: 'terminal', categoryColor: '#8b5cf6', agentName: 'Claude', updatedLabel: '4h ago' },
  { slug: 'HS2-SG1BKJ', title: 'Explore a deliberately long ticket title to verify that every metadata item remains readable in a narrow board column', status: 'backlog', priority: 'low', category: 'investigation', tags: ['client', 'responsive', 'long-tag-example'], categoryIcon: 'search', categoryColor: '#f59e0b', agentName: 'AI', updatedLabel: '1d ago' },
];

export const collectionTickets = signal(initialTickets.map(ticket => ({ ...ticket, tags: [...ticket.tags] })));
export const collectionEvent = signal('Select a ticket or toggle its Up Next star.');

export function resetTicketCollections(): void {
  collectionTickets.value = initialTickets.map(ticket => ({ ...ticket, tags: [...ticket.tags] }));
  collectionEvent.value = 'Select a ticket or toggle its Up Next star.';
}

export function selectCollectionTicket(slug: string, force?: boolean): void {
  collectionTickets.value = collectionTickets.value.map(ticket => ({ ...ticket, selected: ticket.slug === slug ? (force ?? !ticket.selected) : false }));
  const selected = collectionTickets.value.find(ticket => ticket.slug === slug)?.selected;
  collectionEvent.value = `${slug} ${selected ? 'selected' : 'deselected'}`;
}

export function recordCollectionEvent(message: string): void {
  collectionEvent.value = message;
}

export function toggleCollectionTicketUpNext(slug: string): void {
  collectionTickets.value = collectionTickets.value.map(ticket => ticket.slug === slug ? { ...ticket, upNext: !ticket.upNext } : ticket);
  const upNext = collectionTickets.value.find(ticket => ticket.slug === slug)?.upNext;
  collectionEvent.value = `${slug} ${upNext ? 'added to' : 'removed from'} Up Next`;
}

export function TicketListDemo() {
  return <section class="collection-demo" aria-label="TicketList demo">
    <TicketList tickets={collectionTickets.value} label="Example ticket list" />
    <p class="component-stage__event" aria-live="polite">{collectionEvent.value}</p>
    <p class="component-stage__guidance">The list gives TicketRow the full content width for fast scanning while preserving the same selection and metadata contract used in columns.</p>
  </section>;
}

export function TicketBoardDemo() {
  const tickets = collectionTickets.value;
  const columns: TicketColumnProps[] = [
    { id: 'backlog', title: 'Backlog', tickets: tickets.filter(ticket => ticket.status === 'backlog' || ticket.status === 'not_started') },
    { id: 'in-progress', title: 'In progress', tickets: tickets.filter(ticket => ticket.status === 'started') },
    { id: 'done', title: 'Done', tickets: tickets.filter(ticket => ticket.status === 'completed' || ticket.status === 'verified') },
  ];
  return <section class="collection-demo collection-demo--board" aria-label="TicketBoard demo">
    <TicketBoard columns={columns} label="Example status board" />
    <p class="component-stage__event" aria-live="polite">{collectionEvent.value}</p>
    <p class="component-stage__guidance">Each narrow column activates TicketRow’s own card presentation; no TicketCard component or parallel summary markup is involved.</p>
  </section>;
}
