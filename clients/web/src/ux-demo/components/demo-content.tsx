import { AppTab } from '@kerfjs/ui/app-tab';
import { FloatingToolbar } from '@kerfjs/ui/floating-toolbar';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Row } from '@kerfjs/ui/row';
import { TabBar } from '@kerfjs/ui/tab-bar';
import { ToolbarControlGroup } from '@kerfjs/ui/toolbar-control-group';
import { signal } from 'kerfjs';
import { Activity, FolderGit2, Minus, Plus, Terminal } from 'lucide';

import type { ProviderAccount } from '../../api';
import { AiFeedbackDialog } from '../../components/ai-feedback-dialog';
import { AppEmptyState, AppMessageState, ProjectRestoreState } from '../../components/app-empty-state';
import { AppError } from '../../components/app-error';
import { CodexHooksNoticeBanner } from '../../components/codex-hooks-notice-banner';
import { ConversationExportDialog } from '../../components/conversation-export-dialog';
import { KeyboardSettings } from '../../components/keyboard-settings';
import { ManualModelDialog } from '../../components/manual-model-dialog';
import { ProjectCloseDialog } from '../../components/project-close-dialog';
import { ProjectSetupWarningBanner } from '../../components/project-setup-warning-banner';
import { ProviderSetupForm } from '../../components/provider-setup-form';
import { SavedViewDialog } from '../../components/saved-view-dialog';
import { SettingsWorkspace } from '../../components/settings-workspace';
import {
  TerminalCopyDialog,
  type TerminalCopyState,
  TerminalEditMenu,
  type TerminalEditMenuState,
  TerminalPasteDialog,
  type TerminalPasteState,
} from '../../components/terminal-clipboard-dialogs';
import { FixedAspectTerminalCard, TerminalDashboard } from '../../components/terminal-dashboard';
import { TerminalDrawer } from '../../components/terminal-drawer';
import { TerminalKeyBar } from '../../components/terminal-key-bar';
import { TerminalRenameDialog, type TerminalRenameTarget } from '../../components/terminal-rename-dialog';
import { TicketCloseDialog } from '../../components/ticket-close-dialog';
import { TicketLinkChoiceDialog } from '../../components/ticket-link-choice-dialog';
import { TicketPageMore } from '../../components/ticket-page-more';
import { TicketSourceSetupDialog } from '../../components/ticket-source-setup-dialog';
import { AccountsSettings, TicketSourcesSettings } from '../../components/ticket-sources-settings';
import { TrashSettings } from '../../components/trash-settings';
import { NO_TERMINAL_MODIFIERS, type TerminalModifiers } from '../../terminal-keys';
import { AIConversationDemo } from '../ai-conversation-demo';
import { AppLoadingIndicatorDemo } from '../app-loading-indicator-demo';
import {
  AppShellDemo,
  ConnectionStateBannerDemo,
  ProjectTabBarDemo,
  ProjectTabDemo,
  ResizableRegionDemo,
} from '../app-shell-demo';
import { BulkTicketDialogDemo } from '../bulk-ticket-dialog-demo';
import { type DemoDefinition } from '../catalog';
import { CommandRunDialogDemo } from '../command-run-dialog-demo';
import { ConfidenceBadgeDemo } from '../confidence-badge-demo';
import { ConfidenceCalibrationDemo } from '../confidence-calibration-demo';
import { ConnectionDetailsDialogDemo } from '../connection-details-demo';
import {
  AIContentLabelDemo,
  MarkdownEditorDemo,
  MarkdownPreviewDemo,
  NoteCardDemo,
  NoteComposerDemo,
  TicketReaderDemo,
} from '../content-components-demo';
import { ContentTransitionDemo } from '../content-transition-demo';
import {
  DialogHeaderDemo,
  Hs1MigrationBannerDemo,
  Hs1MigrationDialogDemo,
  NotificationsPausedBannerDemo,
  ValueTableDemo,
} from '../dialog-layout-demo';
import { HaltedSessionPopupDemo } from '../halted-session-demo';
import { ListDemo } from '../list-demo';
import { ListHeaderDemo } from '../list-header-demo';
import { ListItemDemo } from '../list-item-demo';
import { NotWorkingDialogDemo, PendingAttachmentPickerDemo } from '../not-working-dialog-demo';
import { NotificationCenterDemo, PermissionRequestDemo } from '../permission-components-demo';
import { ProjectDialogDemo } from '../project-dialog-demo';
import {
  AiToolSettingsDemo,
  CommandNavigationDemo,
  CommandSettingsEditorDemo,
  DriveControlDemo,
  DriveOptionsMenuDemo,
  NotificationNavigationDemo,
  ProjectSidebarDemo,
  ProjectSummaryDemo,
  RepositorySummaryDemo,
  SettingsNavigationDemo,
  TerminalOperationsSidebarDemo,
  ViewNavigationDemo,
} from '../project-sidebar-demo';
import { ProviderIconDemo } from '../provider-icon-demo';
import { ChangeEvidenceDialogDemo, RepositoryStatusPopoverDemo } from '../repository-status-demo';
import { SelectDemo } from '../select-demo';
import { settingsWorkspaceSettings } from '../settings-workspace-demo';
import { StatusBadgeDemo } from '../status-badge-demo';
import { TagChipDemo } from '../tag-chip-demo';
import { TerminalVisibilityDialogDemo } from '../terminal-visibility-demo';
import { TicketBoardColumnDemo, TicketBoardDemo, TicketListDemo } from '../ticket-collections-demo';
import {
  AttachmentGalleryDemo,
  TicketAttachmentsDemo,
  TicketCategorySelectDemo,
  TicketCodeReviewDemo,
  TicketInfoPanelDemo,
  TicketPrioritySelectDemo,
  TicketStatusMenuDemo,
  TicketTimelineDemo,
} from '../ticket-metadata-demo';
import { TicketRowDemo } from '../ticket-row-demo';
import { savedViewDemoSearchModel, TicketSearchFieldDemo } from '../ticket-search-field-demo';
import { ToolbarControlGroupDemo } from '../toolbar-control-group-demo';
import { ToolbarDemo } from '../toolbar-demo';
import { ToolbarTextDemo } from '../toolbar-text-demo';
import {
  PageHeaderDemo,
  QuickTicketComposerDemo,
  TerminalTicketRailDemo,
  TicketInspectorDemo,
  TicketInspectorSkeletonDemo,
  WorkspaceHeaderDemo,
} from '../workspace-components-demo';

export type FormControl = HTMLElement & { checked: boolean; value: string };
/** One project's linked ticket sources and the machine's connection catalog (HS2-3SCH1K). */
export const DEMO_PROJECT_SOURCES = [
  { connectionId: 'git-demo', name: 'Hot Sheet git', provider: 'git', locator: '/work/demo.hs2', default: false },
  {
    connectionId: 'github-main',
    name: 'Product issues',
    provider: 'github',
    locator: 'small-tale/hotsheet2',
    default: true,
    color: '#3b82f6',
  },
  {
    connectionId: 'github-docs',
    name: 'Docs issues',
    provider: 'github',
    locator: 'small-tale/hotsheet-docs',
    default: false,
    color: '#3b82f6',
    disabled: true,
    sharedWith: ['marketing-site'],
  },
];
/** Machine-wide sign-ins (HS2-SM9PM8): a shared GitHub account, an unused one, and a Jira token. */
export const DEMO_ACCOUNTS: ProviderAccount[] = [
  {
    id: 'github-app-01demo',
    provider: 'github',
    host: 'github.com',
    managed: true,
    sources: [
      {
        connection_id: 'github-main',
        name: 'Product issues',
        locator: 'small-tale/hotsheet2',
        disabled: false,
        projects: [{ id: 'demo', alias: 'hotsheet2' }],
      },
      {
        connection_id: 'github-docs',
        name: 'Docs issues',
        locator: 'small-tale/hotsheet-docs',
        disabled: true,
        projects: [
          { id: 'demo', alias: 'hotsheet2' },
          { id: 'marketing', alias: 'marketing-site' },
        ],
      },
    ],
    projects: [
      { id: 'demo', alias: 'hotsheet2' },
      { id: 'marketing', alias: 'marketing-site' },
    ],
  },
  { id: 'github-app-01unused', provider: 'github', host: '', managed: true, sources: [], projects: [] },
  {
    id: 'jira-token',
    provider: 'jira',
    host: 'acme.atlassian.net',
    base_url: 'https://acme.atlassian.net',
    identity: 'dev@acme.test',
    managed: false,
    sources: [
      {
        connection_id: 'jira-ops',
        name: 'Operations',
        locator: 'OPS',
        disabled: false,
        projects: [{ id: 'ops', alias: 'ops-runbooks' }],
      },
    ],
    projects: [{ id: 'ops', alias: 'ops-runbooks' }],
  },
];
/** The drawer demo's installed AI providers: several open the AI shell submenu (HS2-3HT4PA). */
export const TERMINAL_DRAWER_DEMO_PROVIDERS = [
  { id: 'codex', name: 'Codex' },
  { id: 'claude', name: 'Claude' },
] as const;
export const appErrorDemoVisible = signal(true);
export type TicketSourceScenario =
  | 'root'
  | 'signed-out'
  | 'accounts'
  | 'jira-account'
  | 'waiting'
  | 'authorized'
  | 'authorized-existing-default'
  | 'editing'
  | 'editing-shared'
  | 'removing'
  | 'busy'
  | 'remote';
export const ticketSourceScenario = signal<TicketSourceScenario>('root');
export const drawerFocusDemoColumns = signal(60);
// The close dialog's selected running item, so the catalog shows both the chat and the live terminal
// preview (HS2-148B5C).
export const projectCloseDemoSelection = signal('ai-chat:codex-main');
export const keyBarDemoModifiers = signal<TerminalModifiers>(NO_TERMINAL_MODIFIERS),
  keyBarDemoFunctionRow = signal(false),
  keyBarDemoOutput = signal('');
// HS2-FRB545 clipboard sheets: deterministic snapshot text; actions report what production would do.
export const CLIPBOARD_DEMO_TEXT = Array.from(
    { length: 40 },
    (_, index) => `line ${index + 1}: build step ${index + 1} ok`,
  )
    .concat('$ npm test', 'All 1504 tests passed.', '$ ')
    .join('\n'),
  clipboardDemoCopy = signal<TerminalCopyState | undefined>(undefined),
  clipboardDemoPaste = signal<TerminalPasteState | undefined>(undefined),
  clipboardDemoOutput = signal(''),
  clipboardDemoGeneration = signal(0);
// HS2-2Q7KTX: the rename dialog opens renamed (offering Reset to default) or default-named.
export const renameDemoTargets = {
    renamed: { projectId: 'demo', terminalId: 'shell', value: 'Development', defaultName: 'Terminal 1' },
    default: { projectId: 'demo', terminalId: 'shell', value: 'Terminal 1' },
  } as const,
  renameDemoSession = signal(1),
  renameDemoTarget = signal<TerminalRenameTarget | undefined>({ ...renameDemoTargets.renamed, session: 1 }),
  renameDemoOutput = signal('');
// HS2-KKP8YJ long-press edit menu: the stage opens it at a point; items report what production would do.
export const editMenuDemo = signal<TerminalEditMenuState | undefined>(undefined),
  editMenuDemoOutput = signal('');
export const terminalDashboardContextMenu = signal<{ key: string; x: number; y: number } | undefined>(undefined);
export const terminalDashboardHaltCleared = signal(false);

export function demoContent(item: DemoDefinition) {
  if (item.id === 'status-badge') return <StatusBadgeDemo />;
  if (item.id === 'confidence-badge') return <ConfidenceBadgeDemo />;
  if (item.id === 'confidence-calibration') return <ConfidenceCalibrationDemo />;
  if (item.id === 'tag-chip') return <TagChipDemo />;
  if (item.id === 'ticket-row') return <TicketRowDemo />;
  if (item.id === 'ticket-list') return <TicketListDemo />;
  if (item.id === 'ticket-board') return <TicketBoardDemo />;
  if (item.id === 'ticket-board-column') return <TicketBoardColumnDemo />;
  if (item.id === 'workspace-header') return <WorkspaceHeaderDemo />;
  if (item.id === 'page-header') return <PageHeaderDemo />;
  if (item.id === 'quick-ticket-composer') return <QuickTicketComposerDemo />;
  if (item.id === 'ticket-inspector') return <TicketInspectorDemo />;
  if (item.id === 'ticket-inspector-skeleton') return <TicketInspectorSkeletonDemo />;
  if (item.id === 'toolbar-control-group') return <ToolbarControlGroupDemo />;
  if (item.id === 'ticket-search-field') return <TicketSearchFieldDemo />;
  if (item.id === 'toolbar-text') return <ToolbarTextDemo />;
  if (item.id === 'toolbar') return <ToolbarDemo />;
  if (item.id === 'floating-toolbar')
    return (
      <section class="floating-toolbar-demo">
        <p class="floating-toolbar-demo__copy">
          Floating controls remain available over scrolling content without joining the page toolbar.
        </p>
        <FloatingToolbar label="Preview zoom controls" position="bottom-end">
          <ToolbarControlGroup>
            <button type="button" aria-label="Zoom out">
              <LucideIcon icon={Minus} name="minus" />
            </button>
            <button type="button" aria-label="Zoom in">
              <LucideIcon icon={Plus} name="plus" />
            </button>
          </ToolbarControlGroup>
        </FloatingToolbar>
      </section>
    );
  if (item.id === 'dialog-header') return <DialogHeaderDemo />;
  if (item.id === 'value-table') return <ValueTableDemo />;
  if (item.id === 'provider-icon') return <ProviderIconDemo />;
  if (item.id === 'hs1-migration-dialog') return <Hs1MigrationDialogDemo />;
  if (item.id === 'hs1-migration-banner') return <Hs1MigrationBannerDemo />;
  if (item.id === 'codex-hooks-notice-banner') return <CodexHooksNoticeBanner path=".codex/hooks.json" />;
  if (item.id === 'project-setup-warning-banner')
    return (
      <ProjectSetupWarningBanner detail="The development Hot Sheet CLI does not match the current setup templates and may overwrite newer project guidance. Run cargo build -p hotsheet-cli, then reopen the project. No setup files were changed." />
    );
  if (item.id === 'notifications-paused-banner') return <NotificationsPausedBannerDemo />;
  if (item.id === 'halted-session-popup') return <HaltedSessionPopupDemo />;
  if (item.id === 'content-transition') return <ContentTransitionDemo />;
  if (item.id === 'select') return <SelectDemo />;
  if (item.id === 'list') return <ListDemo />;
  if (item.id === 'list-item') return <ListItemDemo />;
  if (item.id === 'list-header') return <ListHeaderDemo />;
  if (item.id === 'ticket-category-select') return <TicketCategorySelectDemo />;
  if (item.id === 'ticket-priority-select') return <TicketPrioritySelectDemo />;
  if (item.id === 'ticket-status-menu') return <TicketStatusMenuDemo />;
  if (item.id === 'ticket-info-panel') return <TicketInfoPanelDemo />;
  if (item.id === 'ticket-timeline') return <TicketTimelineDemo />;
  if (item.id === 'ticket-code-review') return <TicketCodeReviewDemo />;
  if (item.id === 'ticket-attachments') return <TicketAttachmentsDemo />;
  if (item.id === 'attachment-gallery') return <AttachmentGalleryDemo />;
  if (item.id === 'ticket-close-dialog')
    return (
      <TicketCloseDialog
        state={{
          source: {
            id: 'source',
            slug: 'HS2-29MDFH',
            title: 'Repeated UI stability diagnostics',
            projectId: 'kerf',
            projectName: 'Kerf',
            connectionId: 'kerf-git',
            nativeId: 'source',
            qualifiedId: 'kerf-git:source',
          },
          reason: 'duplicate',
          query: 'UI stability',
          candidates: [
            {
              id: 'target',
              slug: 'HS2-8WG3W9',
              title: 'UI stability diagnostics detected render thrashing',
              projectId: 'hotsheet2',
              projectName: 'Hot Sheet 2',
              connectionId: 'hs2-git',
              nativeId: 'target',
              qualifiedId: 'hs2-git:target',
            },
          ],
        }}
      />
    );
  if (item.id === 'project-close-dialog')
    return (
      <ProjectCloseDialog
        state={{
          projectId: 'kerf',
          projectName: 'Kerf',
          resources: [
            {
              kind: 'ai-chat',
              id: 'codex-main',
              name: 'Codex',
              tool: 'codex',
              model: 'gpt-5.6-sol-preview',
              effort: 'high',
              progress: 'Reviewing the resize arbiter…',
              messages: [
                {
                  id: 'm1',
                  role: 'user',
                  content: 'Refactor the terminal resize arbiter to remove the hysteresis race.',
                  sequence: 0,
                },
                {
                  id: 'm2',
                  role: 'assistant',
                  content:
                    'I split the focus-follow decision from the size claim and added a bounded settle window, so a late viewport claim can no longer shrink an already-committed grid.',
                  status: 'completed',
                  sequence: 1,
                  usage: { tokensIn: 1840, tokensOut: 512, costUsd: 0.021, model: 'gpt-5.6-sol-preview' },
                },
              ],
              activity: [
                {
                  id: 'a1',
                  tool: 'codex',
                  kind: 'read',
                  summary: 'Read terminal-sizing.ts',
                  importance: 'normal',
                  sequence: 2,
                },
              ],
              totalUsage: { tokensIn: 1840, tokensOut: 512, costUsd: 0.021, model: 'gpt-5.6-sol-preview' },
            },
            { kind: 'terminal', id: 'tests', name: 'Tests', cwd: '~/code/kerf', progress: 0.6 },
          ],
          selectedKey: projectCloseDemoSelection.value,
        }}
      />
    );
  if (item.id === 'project-dialog') return <ProjectDialogDemo />;
  if (item.id === 'conversation-export-dialog')
    return (
      <ConversationExportDialog
        state={{
          source: {
            conversationId: 'conv-1',
            tool: 'codex',
            sessionId: 'sess-42',
            projectId: 'kerf',
            model: 'gpt-5.6-sol-preview',
            effort: 'high',
            resumable: true,
          },
          messages: [
            {
              id: 'm1',
              role: 'user',
              content: 'Refactor the terminal resize arbiter to remove the hysteresis race.',
              sequence: 0,
            },
            {
              id: 'm2',
              role: 'assistant',
              content: 'I split the focus-follow decision from the size claim and added a bounded settle window.',
              status: 'completed',
              sequence: 1,
              usage: { tokensIn: 1840, tokensOut: 512, costUsd: 0.021, model: 'gpt-5.6-sol-preview' },
            },
            { id: 'm3', role: 'user', content: 'Add a regression test for the late-claim case.', sequence: 2 },
          ],
          draft: {
            scope: { kind: 'all' },
            writeMode: 'create',
            bundle: { includeAttachments: true, includeMedia: true, includeSummary: true },
            destination: {
              selectionToken: 'sel-token',
              displayPath: '~/exports/kerf-resize-arbiter',
              kind: 'directory',
            },
          },
          step: 2,
          summaryAvailable: true,
        }}
      />
    );
  if (item.id === 'command-run-dialog') return <CommandRunDialogDemo />;
  if (item.id === 'bulk-ticket-dialog') return <BulkTicketDialogDemo />;
  if (item.id === 'saved-view-dialog')
    return <SavedViewDialog open mode="create" name="Blocked bugs" searchModel={savedViewDemoSearchModel} />;
  if (item.id === 'ticket-link-choice-dialog')
    return (
      <TicketLinkChoiceDialog
        choice={{
          kind: 'choose',
          reference: { raw: 'HS2-DEMO01', slug: 'HS2-DEMO01' },
          matches: [
            {
              projectId: 'hotsheet2',
              projectName: 'Hot Sheet 2',
              ticketId: '01',
              qualifiedId: 'hs2-git:01',
              connectionId: 'hs2-git',
              slug: 'HS2-DEMO01',
              title: 'Use real project tickets',
              status: 'started',
            },
            {
              projectId: 'kerf',
              projectName: 'Kerf',
              ticketId: 'k1',
              qualifiedId: 'kerf-git:k1',
              connectionId: 'kerf-git',
              slug: 'HS2-DEMO01',
              title: 'Mirror the ticket reference in Kerf',
              status: 'not_started',
            },
          ],
        }}
      />
    );
  if (item.id === 'ai-feedback-dialog')
    return (
      <AiFeedbackDialog
        state={{
          ticketId: 'HS2-DEMO01',
          target: 'note:example',
          rating: 'not_helpful',
          rater: 'demo',
          explanation: 'Please explain the failed step and suggest a smaller fix.',
          revising: false,
        }}
      />
    );
  if (item.id === 'manual-model-dialog')
    return <ManualModelDialog state={{ target: 'settings', providerName: 'Codex', value: 'gpt-5.6-sol-preview' }} />;
  if (item.id === 'keyboard-settings') return <KeyboardSettings overrides={{}} apple={true} />;
  if (item.id === 'ticket-source-setup-dialog') {
    const scenario = ticketSourceScenario.value,
      editing = ['editing', 'editing-shared', 'removing', 'busy'].includes(scenario),
      connection = {
        id: 'github-main',
        provider: 'github',
        locator: 'small-tale/hotsheet2',
        name: 'Product issues',
        default: true,
        settings: {
          attachment_repo: 'small-tale/hotsheet-assets',
          attachment_folder: 'hotsheet-attachments',
          attachment_branch: 'main',
        },
        projects:
          scenario === 'editing-shared'
            ? [
                { id: 'demo', alias: 'Demo project' },
                { id: 'marketing', alias: 'marketing-site' },
              ]
            : [{ id: 'demo', alias: 'Demo project' }],
      };
    return (
      <TicketSourceSetupDialog
        project={{ id: 'demo', root: '/work/demo', name: 'Demo project', stores: [], needsTicketSetup: true }}
        providerKind={
          scenario === 'root' || scenario === 'remote' ? undefined : scenario === 'jira-account' ? 'jira' : 'github'
        }
        providerConnections={editing ? [connection] : []}
        editingProviderId={editing ? connection.id : undefined}
        removingProviderId={scenario === 'removing' ? connection.id : undefined}
        accounts={scenario === 'accounts' || scenario === 'jira-account' ? DEMO_ACCOUNTS : []}
        chosenAccount={scenario === 'jira-account' ? 'jira-token' : undefined}
        projectDefault
        hasProjectDefault={scenario === 'authorized-existing-default'}
        providerBusy={scenario === 'busy'}
        githubAuth={
          scenario === 'waiting'
            ? {
                session: 'demo',
                userCode: 'ABCD-EFGH',
                verificationUri: 'https://github.com/login/device',
                state: 'waiting',
                copied: true,
              }
            : scenario === 'authorized' || scenario === 'authorized-existing-default' || editing
              ? {
                  session: 'demo',
                  userCode: 'ABCD-EFGH',
                  verificationUri: 'https://github.com/login/device',
                  state: 'authorized',
                  repositories: ['small-tale/hotsheet2', 'small-tale/hotsheet-assets'],
                  installations: [{ account: 'small-tale', selection: 'all' }],
                }
              : undefined
        }
        createdGitTicketStore={scenario === 'remote' ? '/work/demo.hs2' : undefined}
        previewScenario={scenario}
        navigation="none"
      />
    );
  }
  if (item.id === 'provider-setup-form')
    return (
      <ProviderSetupForm
        kind="github"
        defaultChoice
        auth={{
          session: 'demo',
          userCode: 'ABCD-EFGH',
          verificationUri: 'https://github.com/login/device',
          state: 'authorized',
          repositories: ['small-tale/hotsheet2'],
        }}
      />
    );
  if (item.id === 'ticket-sources-settings') return <TicketSourcesSettings sources={DEMO_PROJECT_SOURCES} />;
  if (item.id === 'accounts-settings')
    return (
      <>
        <AccountsSettings accounts={DEMO_ACCOUNTS} signingOut={undefined} />
        <AccountsSettings accounts={[]} error="Could not reach the Hot Sheet server." />
      </>
    );
  if (item.id === 'settings-workspace')
    return (
      <SettingsWorkspace
        category={settingsWorkspaceSettings.category.value}
        sources={{ sources: DEMO_PROJECT_SOURCES.slice(0, 1) }}
        accounts={{ accounts: DEMO_ACCOUNTS }}
        ai={{ tools: [], selection: { tool: 'codex' }, loading: false, message: '' }}
        commands={{ commands: [] }}
        lifecycle={{ days: 30, message: '' }}
        terminals={{ inheritGlobalShellHistory: false, message: '' }}
        permissions={{
          automation: { action: settingsWorkspaceSettings.permissionAction.value, delayMs: 60_000 },
          delays: [0, 15_000, 60_000, 120_000],
        }}
        columns={{ hideVerified: false }}
        general={{ showLoadingActivity: true }}
        keyboard={{ overrides: {}, apple: true }}
      />
    );
  if (item.id === 'trash-settings') return <TrashSettings days={14} />;
  if (item.id === 'project-summary') return <ProjectSummaryDemo />;
  if (item.id === 'project-sidebar') return <ProjectSidebarDemo />;
  if (item.id === 'repository-summary') return <RepositorySummaryDemo />;
  if (item.id === 'repository-status-popover') return <RepositoryStatusPopoverDemo />;
  if (item.id === 'change-evidence-dialog') return <ChangeEvidenceDialogDemo />;
  if (item.id === 'connection-details-dialog') return <ConnectionDetailsDialogDemo />;
  if (item.id === 'settings-navigation') return <SettingsNavigationDemo />;
  if (item.id === 'notification-navigation') return <NotificationNavigationDemo />;
  if (item.id === 'view-navigation') return <ViewNavigationDemo />;
  if (item.id === 'command-navigation') return <CommandNavigationDemo />;
  if (item.id === 'command-settings-editor') return <CommandSettingsEditorDemo />;
  if (item.id === 'drive-control') return <DriveControlDemo />;
  if (item.id === 'drive-options-menu') return <DriveOptionsMenuDemo />;
  if (item.id === 'ai-tool-settings') return <AiToolSettingsDemo />;
  if (item.id === 'ai-conversation') return <AIConversationDemo />;
  if (item.id === 'project-tab') return <ProjectTabDemo />;
  if (item.id === 'project-tabs') return <ProjectTabBarDemo />;
  if (item.id === 'app-tab')
    return (
      <section class="app-tab-demo">
        <TabBar id="app-tab-demo" label="Shared application tab demo">
          <AppTab
            id="project"
            name="Project tab"
            selected
            nameOverflow="visible"
            rootAttributes={{ 'data-tab-kind': 'project', 'data-project-id': 'project' }}
            leading={<LucideIcon icon={FolderGit2} name="folder-git-2" />}
          />
          <AppTab
            id="terminal"
            name="Terminal tab"
            className="terminal-tab"
            rootAttributes={{ 'data-tab-kind': 'terminal', 'data-terminal-id': 'terminal' }}
            leading={<LucideIcon icon={Terminal} name="terminal" />}
            trailing={
              <span aria-label="Busy">
                <LucideIcon icon={Activity} name="activity" />
              </span>
            }
          />
        </TabBar>
      </section>
    );
  if (item.id === 'terminal-drawer') {
    const shell = {
      id: 'shell',
      projectId: 'demo',
      projectName: 'Demo project',
      title: 'Development',
      alive: true,
      busy: true,
      scrollback: 'npm run dev\nready on http://127.0.0.1',
    };
    // Phone focus mode is a fixed, viewport-sized surface; each stage contains it (and its blackout
    // backdrop) so the variant can be inspected in place (HS2-01D4JP).
    const focusVariant = (label: string, keyboardVisible: boolean, height: number) => (
      <div class="terminal-drawer-focus-demo__variant">
        <h2>{label}</h2>
        <div class="terminal-drawer-focus-demo__stage" style={`height:${height}px`}>
          <TerminalDrawer
            projectId="demo"
            projectName="Demo project"
            sessions={[shell]}
            width={390}
            height={height}
            fitAcross={2}
            fitHigh={2}
            selectedId="shell"
            focusMode
            focusViewport={{ left: 0, top: 0, width: 390, height }}
            focusTextSize={{
              viewport: { left: 0, top: 0, width: 390, height },
              keyboardVisible,
              columns: drawerFocusDemoColumns.value,
            }}
          />
        </div>
      </div>
    );
    return (
      <div class="terminal-drawer-demos">
        <section class="terminal-drawer-demo">
          <TerminalDrawer
            projectId="demo"
            projectName="Demo project"
            sessions={[shell]}
            width={900}
            height={320}
            fitAcross={2}
            fitHigh={2}
            selectedId="shell"
            contextMenu={terminalDashboardContextMenu.value}
            aiProviders={TERMINAL_DRAWER_DEMO_PROVIDERS}
            defaultAiProvider="claude"
          />
        </section>
        {/* With one installed AI provider the AI shell entry is named for it instead of a submenu (HS2-3HT4PA). */}
        <section class="terminal-drawer-provider-demo" aria-label="One AI provider">
          <h2 class="terminal-drawer-provider-demo__caption">One AI provider</h2>
          <div class="terminal-drawer-provider-demo__stage">
            <TerminalDrawer
              projectId="demo-single"
              projectName="Demo project"
              sessions={[]}
              width={900}
              height={96}
              fitAcross={2}
              fitHigh={2}
              selectedId="grid"
              aiProviders={TERMINAL_DRAWER_DEMO_PROVIDERS.slice(1)}
              defaultAiProvider="claude"
            />
          </div>
        </section>
        {/* Tab status: a halted AI session (HS2-HJ4D1H), and whether an AI session reaches Hot Sheet (HS2-EV1XK3). */}
        <section class="terminal-drawer-provider-demo" aria-label="Terminal tab states">
          <h2 class="terminal-drawer-provider-demo__caption">Terminal tab states</h2>
          <div class="terminal-drawer-provider-demo__stage">
            <TerminalDrawer
              projectId="demo-states"
              projectName="Demo project"
              sessions={[
                {
                  ...shell,
                  id: 'connected',
                  projectId: 'demo-states',
                  title: 'Connected',
                  tool: 'codex',
                  ai_connection: { agent: 'codex', at: '2026-10-05T08:00:00Z' },
                  last_hook_report: {
                    agent: 'codex',
                    at: '2026-10-05T08:00:00Z',
                    source: 'session_start',
                  },
                  aiConnection: 'connected',
                },
                {
                  ...shell,
                  id: 'missing',
                  projectId: 'demo-states',
                  title: 'Not connected',
                  kind: 'ai',
                  tool: 'codex',
                  busy: false,
                  aiConnection: 'missing',
                },
                {
                  ...shell,
                  id: 'halted',
                  projectId: 'demo-states',
                  title: 'Halted',
                  kind: 'ai',
                  tool: 'claude',
                  halt: {
                    error_type: 'overloaded',
                    message: 'Selected model is at capacity. Please try a different model.',
                    at: '2026-10-05T08:00:00Z',
                  },
                },
              ]}
              width={900}
              height={96}
              fitAcross={2}
              fitHigh={2}
              selectedId="connected"
            />
          </div>
        </section>
        <section class="terminal-drawer-provider-demo" aria-label="AI chat tab states">
          <h2 class="terminal-drawer-provider-demo__caption">AI chat tab states</h2>
          <div class="terminal-drawer-provider-demo__stage">
            <TerminalDrawer
              projectId="demo-chat-states"
              projectName="Demo project"
              sessions={[]}
              chatTabs={[
                {
                  id: 'failed-chat',
                  name: 'Stopped chat',
                  tool: 'Codex',
                  error: 'Selected model is at capacity',
                  content: <p>Selected model is at capacity</p>,
                },
                {
                  id: 'recovered-chat',
                  name: 'Recovered chat',
                  tool: 'Claude',
                  content: <p>Retry completed successfully.</p>,
                },
              ]}
              width={900}
              height={96}
              fitAcross={2}
              fitHigh={2}
              selectedId="failed-chat"
            />
          </div>
        </section>
        {/* A touch-first desktop (coarse primary pointer) adds rail Copy and Paste (HS2-5DHHPV). */}
        <section class="terminal-drawer-provider-demo" aria-label="Touch-first desktop">
          <h2 class="terminal-drawer-provider-demo__caption">Touch-first desktop</h2>
          <div class="terminal-drawer-provider-demo__stage">
            <TerminalDrawer
              projectId="demo-touch"
              projectName="Demo project"
              sessions={[{ ...shell, projectId: 'demo-touch' }]}
              width={900}
              height={96}
              fitAcross={2}
              fitHigh={2}
              selectedId="shell"
              touchClipboard
            />
          </div>
        </section>
        <section class="terminal-drawer-focus-demo" aria-label="Phone focus mode variants">
          {focusVariant('Phone focus mode', false, 560)}
          {focusVariant('Phone focus mode, keyboard presented', true, 360)}
        </section>
      </div>
    );
  }
  if (item.id === 'terminal-dashboard')
    return (
      <section class="terminal-dashboard-demo">
        <TerminalDashboard
          groups={[
            {
              projectId: 'demo',
              projectName: 'Demo project',
              sessions: [
                {
                  id: 'shell',
                  projectId: 'demo',
                  projectName: 'Demo project',
                  title: 'Development',
                  tool: 'codex',
                  ai_connection: { agent: 'codex', at: '2026-10-05T08:00:00Z' },
                  last_hook_report: {
                    agent: 'codex',
                    at: '2026-10-05T08:00:00Z',
                    source: 'session_start',
                  },
                  aiConnection: 'connected',
                  alive: true,
                  busy: true,
                  cwd: '/work/demo',
                  progress: 68,
                  scrollback: 'npm run dev\nready on http://127.0.0.1',
                },
                {
                  id: 'tests',
                  projectId: 'demo',
                  projectName: 'Demo project',
                  title: 'Tests',
                  kind: 'ai',
                  tool: 'codex',
                  aiConnection: 'missing',
                  alive: true,
                  busy: false,
                  cwd: '/work/demo',
                  scrollback: '42 tests passed\nwaiting for changes',
                },
                {
                  id: 'halted',
                  projectId: 'demo',
                  projectName: 'Demo project',
                  title: 'Halted AI',
                  tool: 'claude',
                  alive: true,
                  busy: false,
                  aiConnection: 'connected',
                  halt: terminalDashboardHaltCleared.value
                    ? undefined
                    : {
                        error_type: 'overloaded',
                        message: 'Selected model is at capacity.',
                        at: '2026-10-05T08:00:00Z',
                      },
                  scrollback: 'Selected model is at capacity.',
                },
              ],
              chats: [
                {
                  id: 'chat:review',
                  projectId: 'demo',
                  projectName: 'Demo project',
                  name: 'Review chat',
                  tool: 'Codex',
                  busy: true,
                  summary: 'Reviewing the latest workspace changes and test results.',
                },
              ],
            },
          ]}
          width={900}
          height={560}
          fitAcross={3}
          fitHigh={3}
          contextMenu={terminalDashboardContextMenu.value}
        />
      </section>
    );
  if (item.id === 'terminal-operations-sidebar') return <TerminalOperationsSidebarDemo />;
  if (item.id === 'terminal-ticket-rail') return <TerminalTicketRailDemo />;
  if (item.id === 'terminal-key-bar')
    return (
      <section class="terminal-key-bar-demo" aria-label="Terminal key bar variants">
        <div>
          <h2 class="terminal-key-bar-demo__caption">Keys</h2>
          <div class="terminal-key-bar-demo__frame">
            <TerminalKeyBar modifiers={keyBarDemoModifiers.value} functionRow={keyBarDemoFunctionRow.value} />
          </div>
        </div>
        <div>
          <h2 class="terminal-key-bar-demo__caption">Function row</h2>
          <div class="terminal-key-bar-demo__frame">
            <TerminalKeyBar modifiers={{ ctrl: 'once', alt: 'locked', shift: 'off' }} functionRow />
          </div>
        </div>
        <p class="component-stage__event" data-key-bar-demo-output>
          {keyBarDemoOutput.value || 'Tap a key to see the bytes it sends.'}
        </p>
      </section>
    );
  if (item.id === 'fixed-aspect-terminal-card') {
    const session = {
      id: 'shell',
      projectId: 'demo',
      projectName: 'Demo project',
      title: 'Development',
      alive: true,
      busy: true,
      cwd: '/work/demo',
      progress: 68,
      scrollback: 'GNU nano 8.4\n80 columns × 24 rows\n^X Exit',
    };
    return (
      <section class="fixed-aspect-terminal-card-demo" aria-label="Fixed aspect terminal card variants">
        <div>
          <h2 class="fixed-aspect-terminal-card-demo__caption">Grid preview</h2>
          <FixedAspectTerminalCard session={session} fit="aspect" />
        </div>
        <div class="fixed-aspect-terminal-card-demo__magnified">
          <h2 class="fixed-aspect-terminal-card-demo__caption">Magnified interactive</h2>
          <FixedAspectTerminalCard session={session} mode="magnified" fit="aspect" />
        </div>
        <div class="fixed-aspect-terminal-card-demo__magnified">
          <h2 class="fixed-aspect-terminal-card-demo__caption">Magnified, touch-first desktop</h2>
          <FixedAspectTerminalCard session={session} mode="magnified" fit="aspect" touchClipboard />
        </div>
        <div class="fixed-aspect-terminal-card-demo__magnified fixed-aspect-terminal-card-demo__phone">
          <h2 class="fixed-aspect-terminal-card-demo__caption">Magnified phone toolbar</h2>
          <FixedAspectTerminalCard
            session={session}
            mode="magnified"
            fit="aspect"
            mobile={{ viewport: { left: 0, top: 0, width: 390, height: 844 }, keyboardVisible: false, columns: 60 }}
          />
        </div>
        <div class="fixed-aspect-terminal-card-demo__magnified fixed-aspect-terminal-card-demo__phone">
          <h2 class="fixed-aspect-terminal-card-demo__caption">Magnified phone, keyboard presented</h2>
          <FixedAspectTerminalCard
            session={session}
            mode="magnified"
            fit="aspect"
            mobile={{ viewport: { left: 0, top: 0, width: 390, height: 500 }, keyboardVisible: true, columns: 60 }}
          />
        </div>
      </section>
    );
  }
  if (item.id === 'terminal-visibility-dialog') return <TerminalVisibilityDialogDemo />;
  if (item.id === 'terminal-edit-menu')
    return (
      <section class="terminal-edit-menu-demo" aria-label="Terminal edit menu">
        <Row gap="xs">
          <wa-button data-edit-menu-demo-open>Long-press here</wa-button>
          <wa-button data-edit-menu-demo-open="selection">Long-press a word</wa-button>
        </Row>
        <p class="component-stage__event" data-edit-menu-demo-output>
          {editMenuDemoOutput.value || 'Open the menu, then choose an action.'}
        </p>
        <TerminalEditMenu state={editMenuDemo.value} />
      </section>
    );
  if (item.id === 'terminal-copy-dialog' || item.id === 'terminal-paste-dialog') {
    const copy = item.id === 'terminal-copy-dialog';
    return (
      <section class="terminal-clipboard-demo" aria-label={copy ? 'Terminal copy sheet' : 'Terminal paste sheet'}>
        <Row gap="xs">
          {copy ? (
            <wa-button data-clipboard-demo-open="copy">Open copy sheet</wa-button>
          ) : (
            <>
              <wa-button data-clipboard-demo-open="denied">Clipboard denied</wa-button>
              <wa-button data-clipboard-demo-open="unavailable">Clipboard unavailable</wa-button>
            </>
          )}
        </Row>
        <p class="component-stage__event" data-clipboard-demo-output>
          {clipboardDemoOutput.value || 'Open the sheet to try it.'}
        </p>
        {copy ? (
          <TerminalCopyDialog state={clipboardDemoCopy.value} />
        ) : (
          <TerminalPasteDialog state={clipboardDemoPaste.value} />
        )}
      </section>
    );
  }
  if (item.id === 'terminal-rename-dialog')
    return (
      <section class="terminal-rename-demo" aria-label="Terminal rename dialog">
        <Row gap="xs">
          <wa-button data-rename-demo-open="renamed">Rename a renamed terminal</wa-button>
          <wa-button data-rename-demo-open="default">Rename a default-named terminal</wa-button>
        </Row>
        <p class="component-stage__event" data-rename-demo-output>
          {renameDemoOutput.value || 'Reset to default appears only while a rename applies.'}
        </p>
        <TerminalRenameDialog target={renameDemoTarget.value} />
      </section>
    );
  if (item.id === 'resizable-region') return <ResizableRegionDemo />;
  if (item.id === 'connection-state-banner') return <ConnectionStateBannerDemo />;
  if (item.id === 'app-loading-indicator') return <AppLoadingIndicatorDemo />;
  if (item.id === 'app-shell') return <AppShellDemo />;
  if (item.id === 'ticket-page-more')
    return (
      <section aria-label="Ticket page continuation states">
        <TicketPageMore />
        <TicketPageMore loading />
      </section>
    );
  if (item.id === 'app-empty-state')
    return (
      <section class="empty-state-demo-stack" aria-label="Application empty state variants">
        <AppEmptyState />
        <ProjectRestoreState />
        <AppMessageState title="Project unavailable" message="Open another project to continue working." />
      </section>
    );
  if (item.id === 'app-error')
    return (
      <section aria-label="Application error feedback">
        <button type="button" data-app-error-demo-show>
          Show error
        </button>
        {appErrorDemoVisible.value && <AppError message="The project could not be opened. Try again." />}
      </section>
    );
  if (item.id === 'note-card') return <NoteCardDemo />;
  if (item.id === 'note-composer') return <NoteComposerDemo />;
  if (item.id === 'ticket-reader') return <TicketReaderDemo />;
  if (item.id === 'markdown-editor') return <MarkdownEditorDemo />;
  if (item.id === 'markdown-preview') return <MarkdownPreviewDemo />;
  if (item.id === 'ai-content-label') return <AIContentLabelDemo />;
  if (item.id === 'not-working-dialog') return <NotWorkingDialogDemo />;
  if (item.id === 'pending-attachment-picker') return <PendingAttachmentPickerDemo />;
  if (item.id === 'permission-request') return <PermissionRequestDemo />;
  if (item.id === 'notification-center') return <NotificationCenterDemo />;
  return (
    <section class="planned-demo" aria-label={`${item.name} planned demo`}>
      <span>Planned component</span>
      <p>The catalog entry and navigation are ready. Its real component demo will be added in a later slice.</p>
    </section>
  );
}
