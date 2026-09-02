import { signal } from 'kerfjs';

import type { CodeReview } from '../api';
import { PageHeader } from '../components/page-header';
import { QuickTicketComposer } from '../components/quick-ticket-composer';
import { TicketBoard, type TicketColumnProps } from '../components/ticket-board';
import { DEFAULT_TICKET_CATEGORIES } from '../components/ticket-category-select';
import { type InspectorTab,TicketInspector } from '../components/ticket-inspector';
import { TicketList } from '../components/ticket-list';
import type { TicketRowProps } from '../components/ticket-row';
import { applyWorkspaceSortDirection, defaultWorkspaceSortDirection, WorkspaceHeader, type WorkspaceSort, type WorkspaceSortDirection, type WorkspaceViewMode } from '../components/workspace-header';
import { editingNoteId, inspectorBlockedReason, inspectorBlockedReasonDraft, inspectorBlockedReasonEditing, markdownMode, markdownSavedValue, markdownValue, noteDraft, readerNotes } from './content-components-demo';
import { collectionEvent, collectionTickets } from './ticket-collections-demo';

export const workspaceMode = signal<WorkspaceViewMode>('list');
export const workspaceSearchOpen = signal(false);
export const workspaceSearchQuery = signal('');
export const workspaceSort = signal<WorkspaceSort>('updated');
export const workspaceSortDirection = signal<WorkspaceSortDirection>(defaultWorkspaceSortDirection(workspaceSort.value));
export const composerExpanded = signal(false);
export const composerTitle = signal('');
export const composerCategory = signal('task');
export const inspectorOpen = signal(true);
export const inspectorTab = signal<InspectorTab>('info');
export const inspectorCategory = signal('feature');
export const inspectorPriority = signal<TicketRowProps['priority']>('high');
export const inspectorStatus = signal<TicketRowProps['status']>('started');
export const inspectorTitle = signal('Build TicketList and TicketBoard around shared responsive TicketRow');
export const inspectorTitleDraft = signal(inspectorTitle.value);
export const inspectorTitleEditing = signal(false);
export const inspectorTags = signal(['client', 'ux']);
export const inspectorCodeReview: CodeReview = {
  difftool: 'Glassbox',
  truncated: false,
  ranges: [{ from: '92ed71a', to: 'c4a38be', count: 2 }],
  commits: [
    { sha: 'c4a38be', short_sha: 'c4a38be', subject: 'HS2-DEMO: refine the responsive inspector layout', committed_at: '2026-09-02T06:14:00Z' },
    { sha: '92ed71a', short_sha: '92ed71a', subject: 'HS2-DEMO: add ticket-associated commit discovery', committed_at: '2026-09-02T05:42:00Z' },
  ],
};
let demoSequence = 1;

export function focusWorkspaceSearch(root: ParentNode): boolean {
  const input = root.querySelector<HTMLElement>('[name="workspace-search"]');
  if (!input) return false;
  input.focus({ preventScroll: true });
  return true;
}

export function filteredWorkspaceTickets(): TicketRowProps[] {
  const query = workspaceSearchQuery.value.trim().toLocaleLowerCase();
  const tickets = query ? collectionTickets.value.filter(ticket => `${ticket.slug} ${ticket.title} ${ticket.tags.join(' ')}`.toLocaleLowerCase().includes(query)) : collectionTickets.value;
  const priority = { urgent: 0, high: 1, default: 2, low: 3 } as const;
  return [...tickets].sort((left, right) => applyWorkspaceSortDirection(
    workspaceSort.value === 'priority' ? priority[left.priority] - priority[right.priority] || left.slug.localeCompare(right.slug)
      : workspaceSort.value === 'title' ? left.title.localeCompare(right.title) || left.slug.localeCompare(right.slug)
        : workspaceSort.value === 'status' ? left.status.localeCompare(right.status) || left.slug.localeCompare(right.slug)
          : collectionTickets.value.indexOf(right) - collectionTickets.value.indexOf(left),
    workspaceSortDirection.value,
  ));
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
  const category = DEFAULT_TICKET_CATEGORIES.find(choice => choice.value === composerCategory.value)!;
  collectionTickets.value = [{ slug, title, status: 'not_started', priority: 'default', category: category.value, tags: ['new'], selected: true, categoryIcon: category.iconName, categoryColor: category.color, updatedLabel: 'Now' }, ...collectionTickets.value.map(ticket => ({ ...ticket, selected: false }))];
  composerExpanded.value = false;
  composerTitle.value = '';
  collectionEvent.value = `${slug} created`;
  return true;
}

function WorkspaceContent() {
  const tickets = filteredWorkspaceTickets();
  if (workspaceMode.value === 'settings') return <section class="workspace-settings-preview" aria-label="Project settings"><h2>Project settings</h2><p>Configure ticket providers, project defaults, commands, and local checkout behavior for Hot Sheet 2.</p><wa-select label="Default ticket provider" value="git"><wa-option value="git">Hot Sheet git</wa-option><wa-option value="github">GitHub Issues</wa-option></wa-select></section>;
  return workspaceMode.value === 'list' ? <TicketList tickets={tickets} label="Workspace tickets" /> : <TicketBoard columns={workspaceColumns(tickets)} label="Workspace board" />;
}

export function WorkspaceHeaderDemo() {
  return <section class="workspace-component-demo" aria-label="WorkspaceHeader demo">
    <WorkspaceHeader projectName="Hot Sheet 2" mode={workspaceMode.value} searchOpen={workspaceSearchOpen.value} searchQuery={workspaceSearchQuery.value} sort={workspaceSort.value} sortDirection={workspaceSortDirection.value} notificationCount={7} />
    <PageHeader title={workspaceMode.value === 'settings' ? 'Project Settings' : 'Queue'} />
    <div class="workspace-component-demo__content"><WorkspaceContent /></div>
    <p class="component-stage__event" aria-live="polite">{collectionEvent.value}</p>
  </section>;
}

export function PageHeaderDemo() { return <section class="workspace-component-demo" aria-label="PageHeader demo"><PageHeader title="Queue" /><p class="component-stage__event">View identity remains separate from project-level controls.</p></section>; }

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
      ? <TicketInspector slug={ticket.slug} title={inspectorTitle.value} titleEditing={inspectorTitleEditing.value} titleDraft={inspectorTitleDraft.value} status={inspectorStatus.value} priority={inspectorPriority.value} category={inspectorCategory.value} tags={inspectorTags.value} tagSuggestions={['client', 'ux', 'server', 'regression', 'accessibility']} details={markdownValue.value} detailsMode={markdownMode.value} detailsDirty={markdownValue.value !== markdownSavedValue.value} notes={readerNotes.value} editingNoteId={editingNoteId.value} noteDraft={noteDraft.value} blockedReason={inspectorBlockedReason.value} blockedReasonEditing={inspectorBlockedReasonEditing.value} blockedReasonDraft={inspectorBlockedReasonDraft.value} providerName="Hot Sheet git" updatedLabel="Updated now" activeTab={inspectorTab.value} upNext={ticket.upNext} feedbackNeeded={readerNotes.value.some(note => note.kind === 'feedback_needed')} codeReview={inspectorCodeReview} />
      : <wa-button data-action="open-ticket-inspector">Open ticket inspector</wa-button>}
    <p class="component-stage__event" aria-live="polite">{collectionEvent.value}</p>
  </section>;
}
