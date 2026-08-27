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
  { slug: 'HS2-DEMO01', title: 'Plan keyboard movement between board columns', status: 'backlog', priority: 'default', category: 'task', tags: ['client', 'board'], categoryColor: '#14b8a6', agentName: 'AI', updatedLabel: '1d ago' },
  { slug: 'HS2-DEMO02', title: 'Document empty-state behavior for saved views', status: 'not_started', priority: 'low', category: 'task', tags: ['docs', 'views'], categoryColor: '#14b8a6', agentName: 'Claude', updatedLabel: '2d ago' },
  { slug: 'HS2-DEMO03', title: 'Review provider capability messaging', status: 'backlog', priority: 'high', category: 'feature', tags: ['providers', 'ux'], categoryColor: '#8b5cf6', agentName: 'AI', updatedLabel: '2d ago' },
  { slug: 'HS2-DEMO04', title: 'Explore compact attachment summaries', status: 'not_started', priority: 'default', category: 'investigation', tags: ['attachments', 'client'], categoryColor: '#f59e0b', agentName: 'Claude', updatedLabel: '3d ago' },
  { slug: 'HS2-DEMO05', title: 'Specify cross-project drag feedback', status: 'backlog', priority: 'urgent', category: 'feature', tags: ['drag', 'projects'], categoryColor: '#8b5cf6', agentName: 'AI', updatedLabel: '3d ago' },
  { slug: 'HS2-DEMO06', title: 'Implement saved filter chip removal', status: 'started', priority: 'high', category: 'feature', tags: ['filters', 'client'], categoryColor: '#8b5cf6', agentName: 'Codex', updatedLabel: '12m ago' },
  { slug: 'HS2-DEMO07', title: 'Refine command execution progress states', status: 'started', priority: 'default', category: 'task', tags: ['commands', 'ux'], categoryColor: '#14b8a6', agentName: 'Claude', updatedLabel: '24m ago' },
  { slug: 'HS2-DEMO08', title: 'Add connection recovery affordances', status: 'started', priority: 'urgent', category: 'bug', tags: ['server', 'client'], categoryColor: '#ef4444', agentName: 'Codex', updatedLabel: '31m ago' },
  { slug: 'HS2-DEMO09', title: 'Verify responsive inspector transitions', status: 'started', priority: 'default', category: 'task', tags: ['inspector', 'responsive'], categoryColor: '#14b8a6', agentName: 'AI', updatedLabel: '48m ago' },
  { slug: 'HS2-DEMO10', title: 'Polish project switching focus restoration', status: 'started', priority: 'low', category: 'bug', tags: ['projects', 'focus'], categoryColor: '#ef4444', agentName: 'Claude', updatedLabel: '1h ago' },
  { slug: 'HS2-DEMO11', title: 'Ship accessible metadata popup controls', status: 'completed', priority: 'high', category: 'feature', tags: ['metadata', 'a11y'], categoryColor: '#8b5cf6', agentName: 'Codex', updatedLabel: '2h ago' },
  { slug: 'HS2-DEMO12', title: 'Validate repository status summaries', status: 'verified', priority: 'default', category: 'task', tags: ['repository', 'server'], categoryColor: '#14b8a6', agentName: 'AI', updatedLabel: '3h ago' },
  { slug: 'HS2-DEMO13', title: 'Complete context menu icon audit', status: 'completed', priority: 'default', category: 'task', tags: ['menus', 'icons'], categoryColor: '#14b8a6', agentName: 'Claude', updatedLabel: '4h ago' },
  { slug: 'HS2-DEMO14', title: 'Verify toolbar geometry across viewports', status: 'verified', priority: 'low', category: 'investigation', tags: ['toolbar', 'visual'], categoryColor: '#f59e0b', agentName: 'AI', updatedLabel: '5h ago' },
  { slug: 'HS2-DEMO15', title: 'Publish the component coverage matrix', status: 'completed', priority: 'default', category: 'task', tags: ['testing', 'docs'], categoryColor: '#14b8a6', agentName: 'Codex', updatedLabel: '6h ago' },
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
