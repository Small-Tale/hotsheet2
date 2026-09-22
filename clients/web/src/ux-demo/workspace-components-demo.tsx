import '../components/heading.css';

import { Toolbar } from '@kerfjs/ui/toolbar';
import { ToolbarText } from '@kerfjs/ui/toolbar-text';
import { signal } from 'kerfjs';

import type { CodeReview } from '../api';
import { NotificationCenter } from '../components/notification-center';
import { QuickTicketComposer, QuickTicketLauncher } from '../components/quick-ticket-composer';
import { TerminalTicketRail } from '../components/terminal-ticket-rail';
import { TicketBoard, type TicketColumnProps } from '../components/ticket-board';
import { DEFAULT_TICKET_CATEGORIES } from '../components/ticket-category-select';
import { type InspectorTab, TicketInspector } from '../components/ticket-inspector';
import { TicketInspectorSkeleton } from '../components/ticket-inspector-skeleton';
import { TicketList } from '../components/ticket-list';
import type { TicketRowProps } from '../components/ticket-row';
import {
  applyWorkspaceSortDirection,
  defaultWorkspaceSortDirection,
  WorkspaceControls,
  WorkspaceHeader,
  type WorkspaceSort,
  type WorkspaceSortDirection,
  workspaceUpNextState,
  type WorkspaceViewMode,
} from '../components/workspace-header';
import { type PermissionDecision, PermissionInbox, type PermissionScope } from '../permission-notifications';
import { compareWorkspaceTickets } from '../workspace-ticket-sort';
import {
  editingNoteId,
  inspectorBlockedReason,
  inspectorBlockedReasonDraft,
  inspectorBlockedReasonEditing,
  markdownMode,
  markdownSavedValue,
  markdownValue,
  noteDraft,
  readerNotes,
} from './content-components-demo';
import { collectionEvent, collectionTickets } from './ticket-collections-demo';

export const workspaceMode = signal<WorkspaceViewMode>('list');
export const workspaceSearchOpen = signal(false);
export const workspaceSearchQuery = signal('');
export const workspaceSearchHelpOpen = signal(false);
export const workspaceSort = signal<WorkspaceSort>('updated');
export const workspaceSortDirection = signal<WorkspaceSortDirection>(
  defaultWorkspaceSortDirection(workspaceSort.value),
);
export const composerExpanded = signal(false);
export const composerTitle = signal('');
export const composerDetails = signal('');
export const composerCategory = signal('task');
export const composerUpNext = signal(false);
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
  summary: { files: { total: 9, docs: 2, tests: 3, source: 3, other: 1 }, tests_added: 2, tests_modified: 1 },
  difftool: 'Glassbox',
  truncated: false,
  ranges: [{ from: '92ed71a', to: 'c4a38be', count: 2 }],
  commits: [
    {
      sha: 'c4a38be',
      short_sha: 'c4a38be',
      subject: 'HS2-DEMO: refine the responsive inspector layout',
      committed_at: '2026-09-02T06:14:00Z',
    },
    {
      sha: '92ed71a',
      short_sha: '92ed71a',
      subject: 'HS2-DEMO: add ticket-associated commit discovery',
      committed_at: '2026-09-02T05:42:00Z',
    },
  ],
};
let demoSequence = 1;

let workspaceDemoInbox = createWorkspaceDemoInbox();
export const workspaceDemoNotifications = signal({
  pending: workspaceDemoInbox.pending(),
  history: workspaceDemoInbox.history(),
});

function projectWorkspaceDemoNotifications(): void {
  workspaceDemoNotifications.value = {
    pending: workspaceDemoInbox.pending(),
    history: workspaceDemoInbox.history(),
  };
}

function createWorkspaceDemoInbox(now = Date.now()): PermissionInbox {
  const inbox = new PermissionInbox();
  inbox.reconcile(
    { id: 'workspace-demo', name: 'Hot Sheet 2', root: '/demo/hotsheet2', apiPath: '/demo' },
    [
      { id: 1, connection: 'demo-worker', tool: 'Read', action: 'docs/06-clients.md' },
      { id: 2, connection: 'demo-worker', tool: 'Bash', action: 'npm run test:unit', always_allow_supported: true },
      { id: 3, connection: 'demo-worker', tool: 'Edit', action: 'docs/ux-components.md' },
    ],
    [{ id: 'demo-worker', tool: 'Codex', project: 'workspace-demo', role: 'worker', busy: true }],
    now,
  );
  inbox.resolve('workspace-demo:3', 'allow', 'once', false, now);
  return inbox;
}

/** Restore the local notification fixture without sending permission responses to an agent. */
export function resetWorkspaceDemoNotifications(now = Date.now()): void {
  workspaceDemoInbox = createWorkspaceDemoInbox(now);
  projectWorkspaceDemoNotifications();
  collectionEvent.value = 'Demo notifications reset: 2 pending requests.';
}

export function resolveWorkspaceDemoPermission(
  key: string,
  decision: PermissionDecision,
  scope: PermissionScope,
): boolean {
  const request = workspaceDemoInbox.pending().find((item) => item.key === key);
  if (!request || (scope === 'always' && !request.always_allow_supported)) return false;
  if (!workspaceDemoInbox.resolve(key, decision, scope)) return false;
  projectWorkspaceDemoNotifications();
  collectionEvent.value = `${decision === 'allow' ? 'Allowed' : 'Denied'} ${request.tool} request${scope === 'always' ? ' for this kind of request' : ' once'}.`;
  return true;
}

export function ignoreWorkspaceDemoPermission(key: string): boolean {
  if (!workspaceDemoInbox.pending().some((item) => item.key === key)) return false;
  workspaceDemoInbox.ignore(key);
  projectWorkspaceDemoNotifications();
  collectionEvent.value = 'Demo prompt ignored; the request remains pending in Notifications until answered.';
  return true;
}

export function workspaceDemoSelection() {
  const selected = collectionTickets.value.filter((ticket) => ticket.selected);
  return {
    selectedTicketCount: selected.length,
    selectedTicketsUpNext: workspaceUpNextState(selected.map((ticket) => Boolean(ticket.upNext))),
    selectedTicketsUpNextEligible:
      selected.length > 0 && selected.every((ticket) => ticket.status === 'not_started' || ticket.status === 'started'),
  };
}

export function toggleWorkspaceDemoUpNext(): void {
  const selection = workspaceDemoSelection();
  if (!selection.selectedTicketsUpNextEligible) return;
  const upNext = selection.selectedTicketsUpNext !== 'all';
  collectionTickets.value = collectionTickets.value.map((ticket) => (ticket.selected ? { ...ticket, upNext } : ticket));
  collectionEvent.value = `${selection.selectedTicketCount} selected tickets ${upNext ? 'added to' : 'removed from'} Up Next`;
}

export function focusWorkspaceSearch(root: ParentNode): boolean {
  const input = root.querySelector<HTMLElement>('[data-token-search-editor="workspace-search"]');
  if (!input) return false;
  input.focus({ preventScroll: true });
  return true;
}

export function TerminalTicketRailDemo() {
  const mode = workspaceMode.value === 'notifications' ? 'notifications' : 'list';
  return (
    <section class="terminal-ticket-rail-demo">
      <TerminalTicketRail
        projects={[
          { id: 'demo', name: 'Demo project' },
          { id: 'docs', name: 'Documentation and release planning' },
        ]}
        selectedProjectId="demo"
        views={[
          { id: 'all', label: 'Queue' },
          { id: 'backlog', label: 'Backlog' },
          { id: 'archive', label: 'Archive' },
        ]}
        selectedViewId="all"
        controls={
          <WorkspaceControls
            {...workspaceDemoSelection()}
            mode={mode}
            presentation="rail"
            searchOpen={workspaceSearchOpen.value}
            searchQuery={workspaceSearchQuery.value}
            searchHelpOpen={workspaceSearchHelpOpen.value}
            sort={workspaceSort.value}
            sortDirection={workspaceSortDirection.value}
          />
        }
        content={
          mode === 'notifications' ? (
            <NotificationCenter title="Notifications" pending={[]} history={[]} />
          ) : (
            <TicketList tickets={filteredWorkspaceTickets().slice(0, 7)} label="Demo project tickets" />
          )
        }
        inspector={<TicketInspectorDemo />}
        active="root"
        action={<QuickTicketLauncher label="Ticket…" />}
      />
    </section>
  );
}

export function filteredWorkspaceTickets(): TicketRowProps[] {
  const query = workspaceSearchQuery.value.trim().toLocaleLowerCase();
  const tickets = query
    ? collectionTickets.value.filter((ticket) =>
        `${ticket.slug} ${ticket.title} ${ticket.tags.join(' ')}`.toLocaleLowerCase().includes(query),
      )
    : collectionTickets.value;
  return [...tickets].sort((left, right) =>
    workspaceSort.value === 'updated'
      ? applyWorkspaceSortDirection(
          collectionTickets.value.indexOf(right) - collectionTickets.value.indexOf(left),
          workspaceSortDirection.value,
        )
      : compareWorkspaceTickets(left, right, workspaceSort.value, workspaceSortDirection.value),
  );
}

export function workspaceColumns(tickets = filteredWorkspaceTickets()): TicketColumnProps[] {
  return [
    {
      id: 'backlog',
      title: 'Backlog',
      tickets: tickets.filter((ticket) => ticket.status === 'backlog' || ticket.status === 'not_started'),
    },
    { id: 'in-progress', title: 'In progress', tickets: tickets.filter((ticket) => ticket.status === 'started') },
    {
      id: 'done',
      title: 'Done',
      tickets: tickets.filter((ticket) => ticket.status === 'completed' || ticket.status === 'verified'),
    },
  ];
}

export function createDemoTicket(): boolean {
  const title = composerTitle.value.trim();
  if (!title) return false;
  const slug = `HS2-DEMO${demoSequence++}`;
  const category = DEFAULT_TICKET_CATEGORIES.find((choice) => choice.value === composerCategory.value)!;
  collectionTickets.value = [
    {
      slug,
      title,
      status: 'not_started',
      priority: 'default',
      category: category.value,
      tags: ['new'],
      selected: true,
      upNext: composerUpNext.value,
      categoryIcon: category.iconName,
      categoryColor: category.color,
      updatedLabel: 'Now',
    },
    ...collectionTickets.value.map((ticket) => ({ ...ticket, selected: false })),
  ];
  composerExpanded.value = false;
  composerTitle.value = '';
  composerDetails.value = '';
  composerUpNext.value = false;
  collectionEvent.value = `${slug} created`;
  return true;
}

function WorkspaceContent() {
  if (workspaceMode.value === 'notifications')
    return (
      <>
        <NotificationCenter {...workspaceDemoNotifications.value} />
        <wa-button data-action="reset-workspace-notifications">Reset notifications</wa-button>
      </>
    );
  const tickets = filteredWorkspaceTickets();
  if (workspaceMode.value === 'settings')
    return (
      <section class="workspace-settings-preview" aria-label="Project settings">
        <h2>Project settings</h2>
        <p>Configure ticket providers, project defaults, commands, and local checkout behavior for Hot Sheet 2.</p>
        <wa-select label="Default ticket provider" value="git">
          <wa-option value="git">Hot Sheet git</wa-option>
          <wa-option value="github">GitHub Issues</wa-option>
        </wa-select>
      </section>
    );
  return workspaceMode.value === 'list' ? (
    <TicketList tickets={tickets} label="Workspace tickets" />
  ) : (
    <TicketBoard columns={workspaceColumns(tickets)} label="Workspace board" />
  );
}

export function WorkspaceHeaderDemo() {
  return (
    <section class="workspace-component-demo" aria-label="WorkspaceHeader demo">
      <WorkspaceHeader
        {...workspaceDemoSelection()}
        projectName="Hot Sheet 2"
        mode={workspaceMode.value}
        searchOpen={workspaceSearchOpen.value}
        searchQuery={workspaceSearchQuery.value}
        searchHelpOpen={workspaceSearchHelpOpen.value}
        sort={workspaceSort.value}
        sortDirection={workspaceSortDirection.value}
        notificationCount={workspaceDemoNotifications.value.pending.length}
      />
      <div class="app-heading" data-component="heading" data-has-icon="false">
        <Toolbar
          dividerSides=""
          leading={
            <ToolbarText
              text={
                workspaceMode.value === 'settings'
                  ? 'Project Settings'
                  : workspaceMode.value === 'notifications'
                    ? 'Notifications'
                    : 'Queue'
              }
              id="workspace-demo-page-title"
              size="xlarge"
              headingLevel={1}
            />
          }
        />
      </div>
      <div class="workspace-component-demo__content">
        <WorkspaceContent />
      </div>
      <p class="component-stage__event" aria-live="polite">
        {collectionEvent.value}
      </p>
    </section>
  );
}

export function PageHeaderDemo() {
  return (
    <section class="workspace-component-demo" aria-label="PageHeader demo">
      <div class="app-heading" data-component="heading" data-has-icon="false">
        <Toolbar
          dividerSides=""
          leading={<ToolbarText text="Queue" id="page-header-demo-title" size="xlarge" headingLevel={1} />}
        />
      </div>
      <p class="component-stage__event">View identity remains separate from project-level controls.</p>
    </section>
  );
}

export function QuickTicketComposerDemo() {
  return (
    <section class="workspace-component-demo" aria-label="QuickTicketComposer demo">
      <QuickTicketLauncher />
      <QuickTicketComposer
        expanded={composerExpanded.value}
        title={composerTitle.value}
        details={composerDetails.value}
        category={composerCategory.value}
        upNext={composerUpNext.value}
        providerName="Hot Sheet git"
      />
      <TicketList tickets={collectionTickets.value.slice(0, 3)} label="Recently updated tickets" />
      <p class="component-stage__event" aria-live="polite">
        {collectionEvent.value}
      </p>
    </section>
  );
}

export function TicketInspectorDemo() {
  const ticket = collectionTickets.value.find((item) => item.selected) ?? collectionTickets.value[0];
  return (
    <section class="inspector-demo" aria-label="TicketInspector demo">
      {inspectorOpen.value ? (
        <TicketInspector
          slug={ticket.slug}
          title={inspectorTitle.value}
          titleEditing={inspectorTitleEditing.value}
          titleDraft={inspectorTitleDraft.value}
          status={inspectorStatus.value}
          priority={inspectorPriority.value}
          category={inspectorCategory.value}
          tags={inspectorTags.value}
          tagSuggestions={['client', 'ux', 'server', 'regression', 'accessibility']}
          details={markdownValue.value}
          detailsMode={markdownMode.value}
          detailsDirty={markdownValue.value !== markdownSavedValue.value}
          notes={readerNotes.value}
          editingNoteId={editingNoteId.value}
          noteDraft={noteDraft.value}
          blockedReason={inspectorBlockedReason.value}
          blockedReasonEditing={inspectorBlockedReasonEditing.value}
          blockedReasonDraft={inspectorBlockedReasonDraft.value}
          providerName="Hot Sheet git"
          updatedLabel="Updated now"
          activeTab={inspectorTab.value}
          upNext={ticket.upNext}
          feedbackNeeded={readerNotes.value.some((note) => note.kind === 'feedback_needed')}
          codeReview={inspectorCodeReview}
        />
      ) : (
        <wa-button data-action="open-ticket-inspector">Open ticket inspector</wa-button>
      )}
      <p class="component-stage__event" aria-live="polite">
        {collectionEvent.value}
      </p>
    </section>
  );
}

export function TicketInspectorSkeletonDemo() {
  return (
    <section class="inspector-demo" aria-label="TicketInspectorSkeleton demo">
      <TicketInspectorSkeleton slug="HS2-4J50K3" />
    </section>
  );
}
