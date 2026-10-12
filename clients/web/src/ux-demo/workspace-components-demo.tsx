import '../components/heading.css';

import { openPopupMenuAt, type PopupMenuElement } from '@kerfjs/ui/popup-menu';
import { Select } from '@kerfjs/ui/select';
import { Toolbar } from '@kerfjs/ui/toolbar';
import { ToolbarText } from '@kerfjs/ui/toolbar-text';
import { wireNavStack } from '@kerfjs/ui/wire-nav-stack';
import { wireTokenSearchFields } from '@kerfjs/ui/wire-token-search-fields';
import { delegate, delegateCapture, effect, signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import type { CodeReview, DuplicateBacklink } from '../api';
import type { LiveClaimNoticeProps } from '../components/active-claim';
import { NotificationCenter } from '../components/notification-center';
import {
  QuickTicketComposer,
  QuickTicketLauncher,
  showQuickTicketComposer,
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
import { statusMenuAnchorY } from '../components/ticket-status-menu';
import { nextWorkspaceSort, wireWorkspaceOverflowKeyboard } from '../components/workspace-header';
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
import { viewportSafeContextMenuPosition } from '../context-menu-position';
import { orderedSearchText } from '../inline-search';
import { restoreInlineSearchCaret } from '../inline-search-caret';
import { INSPECTOR_AND_EDITOR_ACTIONS } from '../interaction-attrs/inspector-and-editor';
import { wireTicketSearchFields } from '../interactions/ticket-search-field';
import { type PermissionDecision, PermissionInbox, type PermissionScope } from '../permission-notifications';
import { keepSearchOpenForShadowTarget } from '../search-collapse';
import { createTicketSearchModel, inlineSearchTokens } from '../ticket-search-model';
import { compareWorkspaceTickets } from '../workspace-ticket-sort';
import { contextMenu, selectedId } from './components/demo-state';
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
  readerTab,
} from './content-components-demo';
import { DEMO_ACTIONS, DEMO_COMPONENTS, DEMO_FIELDS } from './demo-actions';
import type { DemoModule, FormControl } from './demo-module';
import { syncSettingsControls } from './settings-controls';
import { collectionEvent, collectionTickets, recordCollectionEvent } from './ticket-collections-demo';
import { statusMenuDemoStartedPhase, statusMenuDemoStatus } from './ticket-metadata-demo';
import { TICKET_ROW_CLAIM_ETA } from './ticket-row-demo';
import {
  ticketSearchDemoCollapsibleOpen,
  ticketSearchDemoGrowOpen,
  ticketSearchDemoModel,
  ticketSearchDemoModels,
  ticketSearchDemoRowOpen,
  toggleDemoHelp,
} from './ticket-search-field-demo';

export const workspaceMode = signal<WorkspaceViewMode>('list');
function workspaceHeadingTitle(mode: WorkspaceViewMode): string {
  switch (mode) {
    case 'settings':
      return 'Project Settings';
    case 'notifications':
      return 'Notifications';
    case 'board':
    case 'list':
      return 'Queue';
    default:
      return mode satisfies never;
  }
}
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
  const mode = (() => {
    switch (workspaceMode.value) {
      case 'notifications':
        return 'notifications';
      case 'board':
        return 'board';
      case 'settings':
      case 'list':
        return 'list';
      default:
        return workspaceMode.value satisfies never;
    }
  })();
  const content = (() => {
    switch (mode) {
      case 'notifications':
        return <NotificationCenter title="Notifications" pending={[]} history={[]} inset="flush" />;
      case 'board':
        return (
          <TicketBoard
            columns={railDemoColumns(filteredWorkspaceTickets())}
            label="Demo project board"
            layout="paged"
          />
        );
      case 'list':
        return <TicketList tickets={filteredWorkspaceTickets().slice(0, 7)} label="Demo project tickets" />;
      default:
        return mode satisfies never;
    }
  })();
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
        content={content}
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
              text={workspaceHeadingTitle(workspaceMode.value)}
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
export type InspectorLiveClaimDemo = 'none' | 'estimate' | 'overrun' | 'no-eta' | 'generated-id';
export const inspectorLiveClaim = signal<InspectorLiveClaimDemo>('none');
function demoLiveClaim(): LiveClaimNoticeProps | undefined {
  const state = inspectorLiveClaim.value;
  if (state === 'none') return undefined;
  return {
    agentName: state === 'generated-id' ? 'codex-01M44ZE2BPEZKWTW49DCTATTPW' : 'Claude',
    eta: state === 'no-eta' ? undefined : TICKET_ROW_CLAIM_ETA[state === 'generated-id' ? 'estimate' : state],
  };
}

/** Duplicate states the TicketInspector demo exposes: closed as a duplicate, or the target of others. */
export type InspectorDuplicatesDemo = 'none' | 'target' | 'backlinks' | 'backlinks-partial';
export const inspectorDuplicates = signal<InspectorDuplicatesDemo>('none');
const DEMO_DUPLICATE_BACKLINKS: readonly DuplicateBacklink[] = [
  {
    reference: 'demo:git-local:HS2-DEMO02',
    project_id: 'demo',
    project_name: 'Hot Sheet 2',
    connection_id: 'git-local',
    native_id: 'HS2-DEMO02',
    qualified_id: 'git-local:HS2-DEMO02',
    slug: 'HS2-DEMO02',
    title: 'Document empty-state behavior for saved views',
  },
  {
    reference: 'kerf:git-local:KF-DEMO07',
    project_id: 'kerf',
    project_name: 'Kerf',
    connection_id: 'git-local',
    native_id: 'KF-DEMO07',
    qualified_id: 'git-local:KF-DEMO07',
    slug: 'KF-DEMO07',
    title: 'List rows lose their divider after a duplicate closes',
  },
];
function demoDuplicateProps(): Partial<TicketInspectorProps> {
  switch (inspectorDuplicates.value) {
    case 'none':
      return {};
    case 'target':
      return {
        closeReason: 'duplicate',
        duplicateTarget: {
          id: 'demo:git-local:HS2-DEMO01',
          projectName: 'Hot Sheet 2',
          slug: 'HS2-DEMO01',
          title: 'Plan keyboard movement between board columns',
        },
      };
    case 'backlinks':
      return { duplicateBacklinks: DEMO_DUPLICATE_BACKLINKS };
    case 'backlinks-partial':
      return {
        duplicateBacklinks: DEMO_DUPLICATE_BACKLINKS.slice(0, 1),
        duplicateBacklinkInaccessibleProjects: ['Website'],
      };
  }
}

/** Whether the TicketInspector demo shows a concurrent-edit field conflict (HS2-4GBYYZ). */
export const inspectorFieldConflict = signal<'none' | 'details'>('none');
function demoFieldConflictProps(): Partial<TicketInspectorProps> {
  return inspectorFieldConflict.value === 'details'
    ? {
        fieldConflict: {
          key: 'details',
          field: 'details',
          label: 'Details',
          base: 'Build the reusable metadata presentation.',
          mine: 'Build the reusable metadata and details presentation independently from the inspector shell.',
          theirs: 'Build the reusable metadata presentation, and cover it with browser tests.',
        },
      }
    : {};
}

/** Restore the canonical inspector settings and sync the live controls (HS2-X1SM48). */
export function resetTicketInspectorDemo(root?: ParentNode): void {
  inspectorLiveClaim.value = 'none';
  inspectorDuplicates.value = 'none';
  inspectorFieldConflict.value = 'none';
  if (root)
    syncSettingsControls(root, 'ticket-inspector', {
      values: { 'inspector-live-claim': 'none', 'inspector-duplicates': 'none', 'inspector-field-conflict': 'none' },
    });
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
          { value: 'generated-id', label: 'Generated worker id with ETA' },
        ]}
      />
      <Select
        name="inspector-duplicates"
        label="Duplicates"
        value={inspectorDuplicates.value}
        choices={[
          { value: 'none', label: 'None' },
          { value: 'target', label: 'Closed as a duplicate' },
          { value: 'backlinks', label: 'Duplicated by other tickets' },
          { value: 'backlinks-partial', label: 'Duplicates with an unchecked project' },
        ]}
      />
      <Select
        name="inspector-field-conflict"
        label="Field conflict"
        value={inspectorFieldConflict.value}
        choices={[
          { value: 'none', label: 'None' },
          { value: 'details', label: 'Details changed elsewhere' },
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
    source: { provider: 'git', name: 'Hot Sheet git' },
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
    ...demoDuplicateProps(),
    ...demoFieldConflictProps(),
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

/** Install this file's demo interactions on the catalog root (HS2-HMVBZW). */
function wireWorkspaceComponentsDemo(root: HTMLElement): () => void {
  const demoListeners = createScope();
  // The same shared TicketSearchField wiring the application uses, routed to demo state (HS2-N5G6JS);
  // it is wired before Kerf's so its focus handlers see a chip before Kerf removes or expands it.
  const demoSearchModel = (id: string) =>
    id === 'workspace-search' ? workspaceSearchModel : ticketSearchDemoModel(id);
  demoListeners.add(
    wireTicketSearchFields(root, {
      applyDate: (id, _prefix, value) => {
        demoSearchModel(id)?.commit(value);
      },
      toggleHelp: (id) => {
        if (id === 'workspace-search') workspaceSearchHelpOpen.value = !workspaceSearchHelpOpen.value;
        else toggleDemoHelp(id);
      },
      clear: (id) => {
        if (id === 'workspace-search') {
          workspaceSearchHelpOpen.value = false;
          queueMicrotask(() => {
            focusWorkspaceSearch(root);
          });
        }
      },
      removeToken: () => undefined,
      editToken: () => undefined,
    }),
  );
  // Kerf owns editor chrome, collapsible behavior, and, through the registered models, parsing,
  // chips, suggestions, chip edit/removal, and clear (HS2-5JXBQY).
  demoListeners.add(
    wireTokenSearchFields(root, {
      models: { 'workspace-search': workspaceSearchModel, ...ticketSearchDemoModels },
      // Enter commits a trailing filter through the model; the rebuilt editor gets its caret back at the end.
      onSubmit: ({ id }) => {
        restoreInlineSearchCaret(root, `[data-token-search-editor="${id}"]`);
      },
      collapsible: {
        keepOpenOn: keepSearchOpenForShadowTarget,
        signals: {
          'workspace-search': workspaceSearchOpen,
          'ticket-search-demo-collapsible': ticketSearchDemoCollapsibleOpen,
          'ticket-search-demo-grow': ticketSearchDemoGrowOpen,
          'ticket-search-demo-row': ticketSearchDemoRowOpen,
        },
      },
    }),
  );
  // The TerminalTicketRail demo's NavStack animates push/pop and reports Back through `wireNavStack`
  // (HS2-FY06N4), rewired whenever the demo mounts a new stack.
  let demoRailNavStack: { section: Element; dispose: () => void } | undefined;
  function syncDemoRailNavStack() {
    const section = root.querySelector('.terminal-ticket-rail-demo [data-component="nav-stack"]');
    if (section === (demoRailNavStack?.section ?? null)) return;
    demoRailNavStack?.dispose();
    demoRailNavStack = section
      ? {
          section,
          dispose: wireNavStack(section, {
            onBack: () => {
              terminalRailDemoTicket.value = undefined;
              recordCollectionEvent('Back to the ticket list');
            },
          }),
        }
      : undefined;
  }
  new MutationObserver(syncDemoRailNavStack).observe(root, { childList: true, subtree: true });
  queueMicrotask(syncDemoRailNavStack);
  demoListeners.add(
    delegate(root, 'click', '.workspace-component-demo [data-action="resolve-permission"]', (_event, target) => {
      const { requestKey, decision, scope } = (target as HTMLElement).dataset;
      if (!requestKey || (decision !== 'allow' && decision !== 'deny') || (scope !== 'once' && scope !== 'always'))
        return;
      resolveWorkspaceDemoPermission(requestKey, decision, scope);
    }),
  );
  demoListeners.add(
    delegate(root, 'click', '.workspace-component-demo [data-action="ignore-permission"]', (_event, target) => {
      const key = (target as HTMLElement).dataset.requestKey;
      if (key) ignoreWorkspaceDemoPermission(key);
    }),
  );
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.resetWorkspaceNotifications.selector, () => {
      resetWorkspaceDemoNotifications();
    }),
  );
  demoListeners.add(wireWorkspaceOverflowKeyboard(root));
  function openSelectedDemoTicketActions(target: Element): void {
    const selected = collectionTickets.value.find((ticket) => ticket.selected);
    if (!selected) return;
    const rect = target.getBoundingClientRect();
    contextMenu.value = {
      ...viewportSafeContextMenuPosition(rect.right - 232, rect.bottom, innerWidth, innerHeight, {
        width: 232,
        height: 382,
      }),
      ticketSlug: selected.slug,
    };
  }
  demoListeners.add(delegate(root, 'click', DEMO_ACTIONS.toggleSelectedUpNext.selector, toggleWorkspaceDemoUpNext));
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.openSelectedTicketActions.selector, (_event, target) => {
      openSelectedDemoTicketActions(target);
    }),
  );
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.setViewMode.selector, (_event, target) => {
      const metadata = (target as HTMLElement).dataset;
      workspaceMode.value = (metadata.segmentValue ?? metadata.viewMode) as typeof workspaceMode.value;
      if (workspaceMode.value === 'settings') {
        workspaceSearchOpen.value = false;
        workspaceSearchHelpOpen.value = false;
        workspaceSearchModel.clear();
      }
      recordCollectionEvent(
        `${workspaceMode.value === 'list' ? 'List' : workspaceMode.value === 'board' ? 'Columns' : workspaceMode.value === 'notifications' ? 'Notifications' : 'Settings'} view selected`,
      );
    }),
  );
  demoListeners.add(
    delegate(root, 'click', 'wa-select[name="workspace-sort"] wa-option', (_event, target) => {
      const next = nextWorkspaceSort(
        workspaceSort.value,
        workspaceSortDirection.value,
        (target as FormControl).value as typeof workspaceSort.value,
      );
      workspaceSort.value = next.sort;
      workspaceSortDirection.value = next.direction;
      recordCollectionEvent(`Sorted by ${workspaceSort.value}, ${workspaceSortDirection.value}`);
    }),
  );
  demoListeners.add(
    delegate(root, 'wa-select', '[data-workspace-overflow]', (event) => {
      const item = (event as CustomEvent<{ item: HTMLElement }>).detail.item;
      const action = item.dataset.workspaceOverflowAction;
      if (action === 'toggle-selected-up-next') {
        toggleWorkspaceDemoUpNext();
        return;
      }
      if (action === 'open-selected-ticket-actions') {
        openSelectedDemoTicketActions(item);
        return;
      }
      if (action === 'open-workspace-search') {
        workspaceSearchOpen.value = true;
        queueMicrotask(() => focusWorkspaceSearch(root));
        return;
      }
      if (action === 'set-view-mode') {
        workspaceMode.value = item.dataset.viewMode as typeof workspaceMode.value;
        if (workspaceMode.value === 'settings') {
          workspaceSearchOpen.value = false;
          workspaceSearchHelpOpen.value = false;
          workspaceSearchModel.clear();
        }
        recordCollectionEvent(
          `${workspaceMode.value === 'list' ? 'List' : workspaceMode.value === 'board' ? 'Columns' : workspaceMode.value === 'notifications' ? 'Notifications' : 'Settings'} view selected`,
        );
        return;
      }
      if (action !== 'set-workspace-sort') return;
      const selected = item.dataset.workspaceSort as typeof workspaceSort.value | undefined;
      if (!selected) return;
      const next = nextWorkspaceSort(workspaceSort.value, workspaceSortDirection.value, selected);
      workspaceSort.value = next.sort;
      workspaceSortDirection.value = next.direction;
      recordCollectionEvent(`Sorted by ${workspaceSort.value}, ${workspaceSortDirection.value}`);
    }),
  );
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.toggleFavorite.selector, () => {
      recordCollectionEvent('View favorite toggled');
    }),
  );
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.moreWorkspaceActions.selector, () => {
      recordCollectionEvent('Workspace actions requested');
    }),
  );
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.expandTicketComposer.selector, (_event, target) => {
      if (document.activeElement !== target) (target as HTMLElement).focus({ preventScroll: true });
      composerExpanded.value = true;
      requestAnimationFrame(() => requestAnimationFrame(() => showQuickTicketComposer(root)));
    }),
  );
  demoListeners.add(
    delegateCapture(root, 'wa-after-hide', DEMO_COMPONENTS.quickTicketComposer.selector, (event, target) => {
      if (event.target !== target || !composerExpanded.value) return;
      composerExpanded.value = false;
      composerTitle.value = '';
      composerDetails.value = '';
      composerUpNext.value = false;
      composerSourcePick.value = undefined;
      composerAttachments.value = [];
      recordCollectionEvent('Ticket creation cancelled');
    }),
  );
  demoListeners.add(
    delegate(root, 'input', DEMO_FIELDS.newTicketTitle.selector, (_event, target) => {
      composerTitle.value = (target as FormControl).value;
    }),
  );
  demoListeners.add(
    delegate(root, 'input', DEMO_FIELDS.newTicketDetails.selector, (_event, target) => {
      composerDetails.value = (target as HTMLTextAreaElement).value;
    }),
  );
  demoListeners.add(
    delegate(root, 'change', DEMO_FIELDS.newTicketCategory.selector, (_event, target) => {
      composerCategory.value = (target as FormControl).value;
    }),
  );
  demoListeners.add(
    delegate(root, 'change', DEMO_FIELDS.newTicketSource.selector, (_event, target) => {
      composerSourcePick.value = (target as FormControl).value;
    }),
  );
  demoListeners.add(
    delegate(root, 'change', '[data-settings="ticket-inspector"] [name="inspector-live-claim"]', (_event, target) => {
      inspectorLiveClaim.value = (target as FormControl).value as InspectorLiveClaimDemo;
    }),
  );
  demoListeners.add(
    delegate(root, 'change', '[data-settings="ticket-inspector"] [name="inspector-duplicates"]', (_event, target) => {
      inspectorDuplicates.value = (target as FormControl).value as InspectorDuplicatesDemo;
    }),
  );
  demoListeners.add(
    delegate(
      root,
      'change',
      '[data-settings="ticket-inspector"] [name="inspector-field-conflict"]',
      (_event, target) => {
        inspectorFieldConflict.value = (target as FormControl).value === 'details' ? 'details' : 'none';
      },
    ),
  );
  demoListeners.add(
    delegate(
      root,
      'change',
      '[data-settings="quick-ticket-composer"] [name="composer-source-count"]',
      (_event, target) => {
        composerMultipleSources.value = (target as FormControl).value === 'several';
      },
    ),
  );
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.toggleNewTicketUpNext.selector, () => {
      composerUpNext.value = !composerUpNext.value;
    }),
  );
  let demoAttachmentSequence = 0;
  demoListeners.add(
    delegate(root, 'change', 'input[name="new-ticket-attachments"]', (_event, target) => {
      const input = target as HTMLInputElement;
      composerAttachments.value = [
        ...composerAttachments.value,
        ...Array.from(input.files ?? [], (file) => ({ id: `demo-${demoAttachmentSequence++}`, name: file.name })),
      ];
      input.value = '';
    }),
  );
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.removeNewTicketAttachment.selector, (_event, target) => {
      const id = (target as HTMLElement).dataset.pendingAttachmentId;
      composerAttachments.value = composerAttachments.value.filter((item) => item.id !== id);
    }),
  );
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.clearNewTicketAttachments.selector, () => {
      composerAttachments.value = [];
      recordCollectionEvent('Staged attachments removed');
    }),
  );
  demoListeners.add(
    delegate(root, 'submit', DEMO_ACTIONS.createTicketForm.selector, (event) => {
      event.preventDefault();
      if (!createDemoTicket())
        recordCollectionEvent(
          composerTitle.value.trim()
            ? 'Remove the staged attachments or choose another source'
            : 'Enter a ticket title',
        );
    }),
  );
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.setInspectorTab.selector, (_event, target) => {
      const tab = (target as HTMLElement).dataset.tabId as typeof inspectorTab.value;
      if (selectedId.value === 'ticket-reader') readerTab.value = tab;
      else inspectorTab.value = tab;
    }),
  );
  // The right rail's standard toggle (HS2-QQW6CT) both hides and shows the inspector.
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.toggleTicketInspector.selector, () => {
      inspectorOpen.value = !inspectorOpen.value;
      recordCollectionEvent(inspectorOpen.value ? 'Inspector opened' : 'Inspector closed');
    }),
  );
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.openCodeReview.selector, (_event, target) => {
      const item = target as HTMLElement;
      recordCollectionEvent(
        item.dataset.reviewMode === 'range' ? 'Commit range opened in Glassbox' : 'Commit opened in Glassbox',
      );
    }),
  );
  demoListeners.add(
    delegate(root, 'change', DEMO_FIELDS.inspectorCategory.selector, (_event, target) => {
      inspectorCategory.value = (target as FormControl).value;
    }),
  );
  demoListeners.add(
    delegate(root, 'change', DEMO_FIELDS.inspectorPriority.selector, (_event, target) => {
      inspectorPriority.value = (target as FormControl).value as typeof inspectorPriority.value;
    }),
  );
  demoListeners.add(
    delegate(root, 'click', INSPECTOR_AND_EDITOR_ACTIONS.openInspectorStatusMenu.selector, (_event, target) => {
      const menu = target
        .closest('.ticket-status-menu')
        ?.querySelector<PopupMenuElement>('[data-inspector-status-menu]');
      if (!menu) return;
      const rect = target.getBoundingClientRect();
      openPopupMenuAt(menu, rect.left, statusMenuAnchorY(rect.bottom, window.innerWidth, window.innerHeight));
    }),
  );
  demoListeners.add(
    delegate(root, 'click', INSPECTOR_AND_EDITOR_ACTIONS.setInspectorStatus.selector, (_event, target) => {
      const value = (target as HTMLElement).dataset.ticketStatus;
      if (!value) return;
      if (target.closest('.metadata-control-demo')) {
        statusMenuDemoStatus.value = value as typeof statusMenuDemoStatus.value;
        statusMenuDemoStartedPhase.value = '';
      } else {
        inspectorStatus.value = value as typeof inspectorStatus.value;
        inspectorStartedPhase.value = undefined;
      }
    }),
  );
  demoListeners.add(
    delegate(root, 'click', INSPECTOR_AND_EDITOR_ACTIONS.setInspectorStartedPhase.selector, (_event, target) => {
      const phase = (target as HTMLElement).dataset.startedPhase;
      if (phase === undefined) return;
      if (target.closest('.metadata-control-demo')) {
        statusMenuDemoStatus.value = 'started';
        statusMenuDemoStartedPhase.value = phase as typeof statusMenuDemoStartedPhase.value;
      } else {
        inspectorStatus.value = 'started';
        inspectorStartedPhase.value = (phase || undefined) as typeof inspectorStartedPhase.value;
      }
    }),
  );
  return () => {
    demoListeners.dispose();
  };
}

/** The catalog entries this file renders (HS2-HMVBZW). */
export const demoModule: DemoModule = {
  entries: {
    'workspace-header': { Demo: WorkspaceHeaderDemo },
    'page-header': { Demo: PageHeaderDemo },
    'quick-ticket-composer': {
      Demo: QuickTicketComposerDemo,
      Settings: QuickTicketComposerSettings,
      reset: resetQuickTicketComposerDemo,
    },
    'ticket-inspector': {
      Demo: TicketInspectorDemo,
      Settings: TicketInspectorSettings,
      reset: resetTicketInspectorDemo,
    },
    'ticket-inspector-skeleton': { Demo: TicketInspectorSkeletonDemo },
    'terminal-ticket-rail': { Demo: TerminalTicketRailDemo },
  },
  wire: wireWorkspaceComponentsDemo,
};
