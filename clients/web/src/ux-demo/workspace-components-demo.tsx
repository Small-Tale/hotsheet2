import '../components/heading.css';

import { Select } from '@kerfjs/ui/select';
import { Toolbar } from '@kerfjs/ui/toolbar';
import { ToolbarText } from '@kerfjs/ui/toolbar-text';
import { effect, signal } from 'kerfjs';

import type { CodeReview } from '../api';
import type { LiveClaimNoticeProps } from '../components/active-claim';
import { NotificationCenter } from '../components/notification-center';
import {
  QuickTicketComposer,
  QuickTicketLauncher,
  strandedNewTicketAttachments,
} from '../components/quick-ticket-composer';
import { TerminalTicketRail } from '../components/terminal-ticket-rail';
import { TicketBoard, type TicketColumnProps } from '../components/ticket-board';
import { DEFAULT_TICKET_CATEGORIES } from '../components/ticket-category-select';
import {
  type InspectorTab,
  TicketInspector,
  ticketInspectorPanel,
  type TicketInspectorProps,
  TicketInspectorSkeleton,
} from '../components/ticket-inspector';
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
import { orderedSearchText } from '../inline-search';
import { type PermissionDecision, PermissionInbox, type PermissionScope } from '../permission-notifications';
import { createTicketSearchModel, inlineSearchTokens } from '../ticket-search-model';
import { compareWorkspaceTickets } from '../workspace-ticket-sort';
import {
  editingNoteId,
  inspectorBlockedReason,
  inspectorBlockedReasonDraft,
  inspectorBlockedReasonEditing,
  latestDemoConfidence,
  markdownMode,
  markdownSavedValue,
  markdownValue,
  noteDraft,
  readerNotes,
} from './content-components-demo';
import { syncSettingsControls } from './settings-controls';
import { collectionEvent, collectionTickets } from './ticket-collections-demo';
import { TICKET_ROW_CLAIM_ETA } from './ticket-row-demo';

export const workspaceMode = signal<WorkspaceViewMode>('list');
/** The ticket pushed onto the TerminalTicketRail demo's NavStack (HS2-FY06N4), if any. */
export const terminalRailDemoTicket = signal<string | undefined>(undefined);
export const workspaceSearchOpen = signal(false);
/** The demo's Kerf-managed workspace search model (HS2-5JXBQY); the tags are the collection's. */
export const workspaceSearchModel = createTicketSearchModel({
  tags: () => [...new Set(collectionTickets.value.flatMap((ticket) => ticket.tags))].sort((a, b) => a.localeCompare(b)),
});
/** The readable search expression the model holds, for the demo's own row filtering. */
export const workspaceSearchQuery = signal('');
effect(() => {
  const state = workspaceSearchModel.state.value;
  workspaceSearchQuery.value = orderedSearchText(state.query, inlineSearchTokens(state), () => true);
});
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
/** Writable ticket sources the demo composer can target (HS2-NZMJBJ). */
export const composerSources = [
  { value: 'git-local', label: 'Hot Sheet git', attachments: true },
  // GitHub issues cannot take attachments, so files staged for git stay listed but block Create
  // until removed or the user switches back (HS2-8HHHK3).
  { value: 'github-issues', label: 'GitHub issues', attachments: false },
] as const;
/** Demo files staged in the composer (names only; nothing is uploaded). */
export const composerAttachments = signal<readonly { id: string; name: string }[]>([]);
/** Whether the demo project has several writable sources (the Select) or one (plain text). */
export const composerMultipleSources = signal(true);
/** The demo's in-memory "most recently used" source, which the next composer preselects. */
export const composerSource = signal<string>(composerSources[0].value);
/** The source picked in the open composer; creating remembers it, cancelling forgets it. */
export const composerSourcePick = signal<string | undefined>(undefined);
export const demoComposerTarget = () => composerSourcePick.value ?? composerSource.value;
const demoComposerSource = () =>
  composerMultipleSources.value
    ? composerSources.find((item) => item.value === demoComposerTarget())!
    : composerSources[0];
export const inspectorOpen = signal(true);
export const inspectorTab = signal<InspectorTab>('info');
export const inspectorCategory = signal('feature');
export const inspectorPriority = signal<TicketRowProps['priority']>('high');
export const inspectorStatus = signal<TicketRowProps['status']>('started');
export const inspectorStartedPhase = signal<TicketRowProps['startedPhase']>('planning');
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
  const mode =
    workspaceMode.value === 'notifications' ? 'notifications' : workspaceMode.value === 'board' ? 'board' : 'list';
  return (
    <section class="terminal-ticket-rail-demo">
      <TerminalTicketRail
        collapseControl
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
            searchModel={workspaceSearchModel}
            searchHelpOpen={workspaceSearchHelpOpen.value}
            sort={workspaceSort.value}
            sortDirection={workspaceSortDirection.value}
          />
        }
        content={
          mode === 'notifications' ? (
            <NotificationCenter title="Notifications" pending={[]} history={[]} inset="flush" />
          ) : mode === 'board' ? (
            <TicketBoard
              columns={railDemoColumns(filteredWorkspaceTickets())}
              label="Demo project board"
              layout="paged"
            />
          ) : (
            <TicketList tickets={filteredWorkspaceTickets().slice(0, 7)} label="Demo project tickets" />
          )
        }
        // Selecting a ticket pushes its detail onto the rail's NavStack; Back pops it (HS2-FY06N4).
        detail={
          terminalRailDemoTicket.value
            ? {
                key: `ticket:${terminalRailDemoTicket.value}`,
                parts: ticketInspectorPanel(demoInspectorProps(terminalRailDemoTicket.value)),
              }
            : undefined
        }
        action={{ kind: 'new-ticket', label: 'Ticket…', size: 'compact' }}
      />
    </section>
  );
}

/** The rail's paged columns for the demo tickets: one status column per group, every ticket shown. */
function railDemoColumns(tickets: readonly TicketRowProps[]) {
  return (
    [
      ['not-started', 'Not Started', 'not_started'],
      ['started', 'Started', 'started'],
      ['completed', 'Completed', 'completed'],
      ['verified', 'Verified', 'verified'],
    ] as const
  ).map(([id, title, status]) => {
    const rows = tickets.filter((ticket) => ticket.status === status);
    return { id, title, tickets: rows, totalCount: rows.length };
  });
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
  if (strandedNewTicketAttachments(composerAttachments.value.length, demoComposerSource().attachments)) return false;
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
  composerAttachments.value = [];
  const source = demoComposerSource();
  composerSource.value = source.value;
  composerSourcePick.value = undefined;
  collectionEvent.value = `${slug} created in ${source.label}`;
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
        <h2 class="workspace-settings-preview__title">Project settings</h2>
        <p class="workspace-settings-preview__description">
          Configure ticket providers, project defaults, commands, and local checkout behavior for Hot Sheet 2.
        </p>
        <Select
          name="default-ticket-provider"
          label="Default ticket provider"
          value="git"
          choices={[
            { value: 'git', label: 'Hot Sheet git' },
            { value: 'github', label: 'GitHub Issues' },
          ]}
        />
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
        searchModel={workspaceSearchModel}
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

/** Restore the canonical composer settings and sync the live controls (HS2-X1SM48). */
export function resetQuickTicketComposerDemo(root?: ParentNode): void {
  composerMultipleSources.value = true;
  if (root) syncSettingsControls(root, 'quick-ticket-composer', { values: { 'composer-source-count': 'several' } });
}

export function QuickTicketComposerSettings() {
  return (
    <form class="settings-form" data-settings="quick-ticket-composer">
      <Select
        name="composer-source-count"
        label="Writable ticket sources"
        value={composerMultipleSources.value ? 'several' : 'one'}
        choices={[
          { value: 'one', label: 'One (plain text)' },
          { value: 'several', label: 'Several (source Select)' },
        ]}
      />
      <wa-button type="button" data-action="reset-settings">
        Reset
      </wa-button>
    </form>
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
        providerName={demoComposerSource().label}
        defaultSourceName={composerSources[0].label}
        sourceIsDefault={demoComposerSource().value === composerSources[0].value}
        sources={composerMultipleSources.value ? composerSources : composerSources.slice(0, 1)}
        source={demoComposerSource().value}
        attachments={composerAttachments.value}
        attachmentsEnabled={demoComposerSource().attachments}
      />
      <TicketList tickets={collectionTickets.value.slice(0, 3)} label="Recently updated tickets" />
      <p class="component-stage__event" aria-live="polite">
        {collectionEvent.value}
      </p>
    </section>
  );
}

/** Live-claim header states the TicketInspector demo exposes (HS2-QKNQXC). */
export type InspectorLiveClaimDemo = 'none' | 'estimate' | 'overrun' | 'no-eta';
export const inspectorLiveClaim = signal<InspectorLiveClaimDemo>('none');
function demoLiveClaim(): LiveClaimNoticeProps | undefined {
  const state = inspectorLiveClaim.value;
  if (state === 'none') return undefined;
  return { agentName: 'Claude', eta: state === 'no-eta' ? undefined : TICKET_ROW_CLAIM_ETA[state] };
}

/** Restore the canonical inspector settings and sync the live controls (HS2-X1SM48). */
export function resetTicketInspectorDemo(root?: ParentNode): void {
  inspectorLiveClaim.value = 'none';
  if (root) syncSettingsControls(root, 'ticket-inspector', { values: { 'inspector-live-claim': 'none' } });
}

export function TicketInspectorSettings() {
  return (
    <form class="settings-form" data-settings="ticket-inspector">
      <Select
        name="inspector-live-claim"
        label="Live claim (AI working)"
        value={inspectorLiveClaim.value}
        choices={[
          { value: 'none', label: 'None' },
          { value: 'estimate', label: 'With ETA estimate' },
          { value: 'overrun', label: 'Past its ETA (Soon)' },
          { value: 'no-eta', label: 'Without an ETA' },
        ]}
      />
      <wa-button type="button" data-action="reset-settings">
        Reset
      </wa-button>
    </form>
  );
}

/** `collapseControl` mirrors the rail's standard toggle; the terminal rail's pushed detail omits it. */
/** The demo inspector's props for `slug` (the selected demo ticket by default). */
function demoInspectorProps(slug?: string): TicketInspectorProps {
  const ticket =
    collectionTickets.value.find((item) => item.slug === slug) ??
    collectionTickets.value.find((item) => item.selected) ??
    collectionTickets.value[0];
  return {
    slug: ticket.slug,
    title: inspectorTitle.value,
    titleEditing: inspectorTitleEditing.value,
    titleDraft: inspectorTitleDraft.value,
    status: inspectorStatus.value,
    startedPhase: inspectorStartedPhase.value,
    canEditStartedPhase: true,
    priority: inspectorPriority.value,
    category: inspectorCategory.value,
    tags: inspectorTags.value,
    tagSuggestions: ['client', 'ux', 'server', 'regression', 'accessibility'],
    details: markdownValue.value,
    detailsMode: markdownMode.value,
    detailsDirty: markdownValue.value !== markdownSavedValue.value,
    notes: readerNotes.value,
    editingNoteId: editingNoteId.value,
    noteDraft: noteDraft.value,
    blockedReason: inspectorBlockedReason.value,
    blockedReasonEditing: inspectorBlockedReasonEditing.value,
    blockedReasonDraft: inspectorBlockedReasonDraft.value,
    providerName: 'Hot Sheet git',
    updatedLabel: 'Updated now',
    activeTab: inspectorTab.value,
    upNext: ticket.upNext,
    feedbackNeeded: readerNotes.value.some((note) => note.kind === 'feedback_needed'),
    latestConfidence: latestDemoConfidence(readerNotes.value),
    liveClaim: demoLiveClaim(),
    codeReview: inspectorCodeReview,
  };
}

export function TicketInspectorDemo({ collapseControl = true }: { collapseControl?: boolean } = {}) {
  return (
    <section class="inspector-demo" aria-label="TicketInspector demo">
      {inspectorOpen.value ? (
        <TicketInspector {...demoInspectorProps()} collapseControl={collapseControl} />
      ) : (
        <wa-button data-action="toggle-ticket-inspector">Open ticket inspector</wa-button>
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
      <TicketInspectorSkeleton slug="HS2-4J50K3" collapseControl />
    </section>
  );
}
