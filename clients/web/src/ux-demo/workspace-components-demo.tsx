import { signal } from 'kerfjs';
import { WorkspaceHeader, type WorkspaceViewMode } from '../components/workspace-header';
import { QuickTicketComposer } from '../components/quick-ticket-composer';
import { TicketInspector, type InspectorTab } from '../components/ticket-inspector';
import { TicketList } from '../components/ticket-list';
import { TicketBoard, type TicketColumnProps } from '../components/ticket-board';
import type { TicketRowProps } from '../components/ticket-row';
import { collectionEvent, collectionTickets } from './ticket-collections-demo';

export const workspaceMode = signal<WorkspaceViewMode>('list');
export const workspaceSearchOpen = signal(false);
export const workspaceSearchQuery = signal('');
export const composerExpanded = signal(false);
export const composerTitle = signal('');
export const composerCategory = signal('task');
export const inspectorOpen = signal(true);
export const inspectorTab = signal<InspectorTab>('info');
let demoSequence = 1;

export function filteredWorkspaceTickets(): TicketRowProps[] {
  const query = workspaceSearchQuery.value.trim().toLocaleLowerCase();
  return query ? collectionTickets.value.filter(ticket => `${ticket.slug} ${ticket.title} ${ticket.tags.join(' ')}`.toLocaleLowerCase().includes(query)) : collectionTickets.value;
}

export function workspaceColumns(tickets = filteredWorkspaceTickets()): TicketColumnProps[] {
  return [
    { id: 'backlog', title: 'Backlog', tickets: tickets.filter(ticket => ticket.status === 'backlog' || ticket.status === 'not_started') },
    { id: 'in-progress', title: 'In progress', tickets: tickets.filter(ticket => ticket.status === 'started') },
    { id: 'done', title: 'Done', tickets: tickets.filter(ticket => ticket.status === 'completed' || ticket.status === 'verified') },
  ];
}

export function createDemoTicket(): boolean {
  const title = composerTitle.value.trim();
  if (!title) return false;
  const slug = `HS2-DEMO${demoSequence++}`;
  collectionTickets.value = [{ slug, title, status: 'not_started', priority: 'default', category: composerCategory.value, tags: ['new'], selected: true, categoryColor: '#3b82f6', updatedLabel: 'Now' }, ...collectionTickets.value.map(ticket => ({ ...ticket, selected: false }))];
  composerExpanded.value = false;
  composerTitle.value = '';
  collectionEvent.value = `${slug} created`;
  return true;
}

function WorkspaceContent() {
  const tickets = filteredWorkspaceTickets();
  return workspaceMode.value === 'list' ? <TicketList tickets={tickets} label="Workspace tickets" /> : <TicketBoard columns={workspaceColumns(tickets)} label="Workspace board" />;
}

export function WorkspaceHeaderDemo() {
  return <section class="workspace-component-demo" aria-label="WorkspaceHeader demo">
    <WorkspaceHeader projectName="Hot Sheet 2" viewName="All Tickets" mode={workspaceMode.value} searchOpen={workspaceSearchOpen.value} searchQuery={workspaceSearchQuery.value} />
    <div class="workspace-component-demo__content"><WorkspaceContent /></div>
    <p class="component-stage__event" aria-live="polite">{collectionEvent.value}</p>
  </section>;
}

export function QuickTicketComposerDemo() {
  return <section class="workspace-component-demo" aria-label="QuickTicketComposer demo">
    <QuickTicketComposer expanded={composerExpanded.value} title={composerTitle.value} category={composerCategory.value} providerName="Hot Sheet git" />
    <TicketList tickets={collectionTickets.value.slice(0, 3)} label="Recently updated tickets" />
    <p class="component-stage__event" aria-live="polite">{collectionEvent.value}</p>
  </section>;
}

export function TicketInspectorDemo() {
  const ticket = collectionTickets.value.find(item => item.selected) ?? collectionTickets.value[0];
  return <section class="inspector-demo" aria-label="TicketInspector demo">
    {inspectorOpen.value
      ? <TicketInspector slug={ticket.slug} title={ticket.title} status={ticket.status} priority={ticket.priority} category={ticket.category} tags={ticket.tags} details="This is a focused summary of the ticket details. The production client will render Markdown and preserve in-progress edits when opening the larger reader." activeTab={inspectorTab.value} upNext={ticket.upNext} />
      : <wa-button data-action="open-ticket-inspector">Open ticket inspector</wa-button>}
    <p class="component-stage__event" aria-live="polite">{collectionEvent.value}</p>
  </section>;
}
