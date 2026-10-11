import type {
  AiToolDefaults,
  CheckoutTicketCounts,
  CommandDefinition,
  CommandRun,
  CorruptTicket,
  CustomView,
  RepositoryStatus,
  TicketRow as WireTicketRow,
  ToolConnection,
} from '../api';
import type { AiToolDescriptor } from '../components/drive-options-menu';
import type { NotificationView } from '../components/notification-navigation';
import type { SettingsCategory } from '../components/settings-navigation';
import type { SidebarSurfaceProps } from '../components/workspace-composition-surfaces';
import type { WorkspaceViewMode } from '../components/workspace-header';
import type { Project } from '../interactions/types';
import { projectChatConnectionId, projectDriveControlState } from '../project-drive';
import { ticketCompletionTrend } from '../ticket-completion-trend';
import { customTicketViewId, type TicketView } from '../ticket-views';
import type { SidebarSearchCounts } from './workspace-search';

/** A signal as the sidebar projection reads it. */
interface Readable<T> {
  readonly value: T;
}

/** Live application bindings the project sidebar projection reads (HS2-BSSWGV). */
export interface SidebarSurfaceDependencies {
  viewMode: Readable<WorkspaceViewMode>;
  notificationView: Readable<NotificationView>;
  notificationsPaused: Readable<boolean>;
  partialSourcesByProject: Readable<Record<string, boolean>>;
  sidebarSearchCounts: Readable<SidebarSearchCounts | undefined>;
  repository: Readable<RepositoryStatus | null>;
  repositoryError: Readable<string>;
  tickets: Readable<WireTicketRow[]>;
  selectedView: Readable<TicketView>;
  corruptTickets: Readable<CorruptTicket[]>;
  driveConnectionsByProject: Readable<Record<string, ToolConnection[]>>;
  drivePendingByProject: Readable<Record<string, boolean>>;
  commandDefinitions: Readable<CommandDefinition[]>;
  commandGroupExpanded: Readable<boolean>;
  commandGroupsCollapsed: Readable<Record<string, string[]>>;
  driveOptionsOpen: Readable<boolean>;
  driveOptionsAnchor: Readable<{ x: number; y: number } | undefined>;
  aiTools: Readable<AiToolDescriptor[]>;
  aiSettingsLoading: Readable<boolean>;
  aiSettingsMessage: Readable<string>;
  driveOverridesByProject: Readable<Record<string, Partial<AiToolDefaults>>>;
  aiDefaults: Readable<AiToolDefaults>;
  conversationOpen: Readable<boolean>;
  conversationConnectionId: Readable<string | undefined>;
  project: () => Project | undefined;
  settingsCategory: () => SettingsCategory;
  projectPermissionHistory: (projectId: string) => ReadonlyArray<{ resolvedAt: number }>;
  projectPendingPermissions: (projectId: string) => readonly unknown[];
  projectTicketCounts: (projectId: string) => CheckoutTicketCounts;
  searchBarActive: () => boolean;
  searchSignature: () => string;
  customViewsFor: (projectId: string) => CustomView[];
  effectiveDriveSelection: (projectId: string) => AiToolDefaults;
  commandRunFor: (commandId: string) => CommandRun | undefined;
  commandIcon: (command: CommandDefinition) => string;
  aiToolLabel: (tool: string) => string;
}

/**
 * Projects the left sidebar for the selected project: the Settings and Notifications navigations,
 * or the project sidebar's stats, repository summary, ticket views (with live search counts),
 * commands, and Drive/chat controls. Extracted from the application runtime without behavior
 * changes (HS2-BSSWGV).
 */
export function createSidebarSurface(dependencies: SidebarSurfaceDependencies) {
  const {
    viewMode,
    notificationView,
    notificationsPaused,
    partialSourcesByProject,
    sidebarSearchCounts,
    repository,
    repositoryError,
    tickets,
    selectedView,
    corruptTickets,
    driveConnectionsByProject,
    drivePendingByProject,
    commandDefinitions,
    commandGroupExpanded,
    commandGroupsCollapsed,
    driveOptionsOpen,
    driveOptionsAnchor,
    aiTools,
    aiSettingsLoading,
    aiSettingsMessage,
    driveOverridesByProject,
    aiDefaults,
    conversationOpen,
    conversationConnectionId,
    project,
    settingsCategory,
    projectPermissionHistory,
    projectPendingPermissions,
    projectTicketCounts,
    searchBarActive,
    searchSignature,
    customViewsFor,
    effectiveDriveSelection,
    commandRunFor,
    commandIcon,
    aiToolLabel,
  } = dependencies;
  function sidebarSurfaceProps(): SidebarSurfaceProps {
    const current = project()!;
    if (viewMode.value === 'settings') return { kind: 'settings', selected: settingsCategory() };
    if (viewMode.value === 'notifications') {
      const history = projectPermissionHistory(current.id),
        now = Date.now();
      return {
        kind: 'notifications',
        selected: notificationView.value,
        counts: {
          pending: projectPendingPermissions(current.id).length,
          day: history.filter((item) => item.resolvedAt >= now - 24 * 60 * 60 * 1000).length,
          week: history.length,
        },
        paused: notificationsPaused.value,
      };
    }
    const counts = projectTicketCounts(current.id),
      countsPartial = partialSourcesByProject.value[current.id] ?? false,
      searchCounts = sidebarSearchCounts.value,
      searchActive =
        searchBarActive() && searchCounts?.projectId === current.id && searchCounts.signature === searchSignature(),
      searchView = (id: TicketView, fallback?: number) => ({
        count: searchActive ? searchCounts.values[id] : fallback,
        countLoading: searchActive && searchCounts.pending.includes(id),
        searchCount: searchActive && !searchCounts.pending.includes(id) && searchCounts.values[id] !== undefined,
        countPartial: !searchActive && countsPartial,
      }),
      repo = repository.value,
      completionTrend = counts.completion_trend ?? ticketCompletionTrend(tickets.value),
      views = [
        { id: 'all', label: 'Queue', ...searchView('all', counts.queued), icon: 'all' as const },
        { id: 'backlog', label: 'Backlog', ...searchView('backlog', counts.backlog), icon: 'backlog' as const },
        { id: 'archive', label: 'Archive', ...searchView('archive', counts.archive), icon: 'archive' as const },
        ...((counts.trash ?? 0) > 0 || selectedView.value === 'trash'
          ? [
              {
                id: 'trash' as const,
                label: 'Trash',
                count: counts.trash ?? 0,
                countPartial: countsPartial,
                icon: 'trash' as const,
              },
            ]
          : []),
        ...customViewsFor(current.id).map((view) => {
          const id = customTicketViewId(view.id);
          return { id, label: view.name, ...searchView(id), icon: 'custom' as const, manageable: true };
        }),
        ...(corruptTickets.value.length
          ? [
              {
                id: 'errors',
                label: 'Ticket errors',
                count: corruptTickets.value.length,
                attention: true,
                icon: 'errors' as const,
              },
            ]
          : []),
      ],
      selection = effectiveDriveSelection(current.id),
      tool = selection.tool,
      drive = projectDriveControlState(
        driveConnectionsByProject.value[current.id] ?? [],
        current.id,
        drivePendingByProject.value[current.id] ?? false,
        tool,
      );
    return {
      kind: 'project',
      sidebar: {
        completedToday: counts.completed_today,
        inProgress: counts.started,
        completionTrend,
        branch: repo?.branch ?? 'No git branch',
        unpushed: repo?.ahead ?? 0,
        behind: repo?.behind ?? 0,
        uncommitted: (repo?.staged ?? 0) + (repo?.unstaged ?? 0) + (repo?.untracked ?? 0),
        conflicted: repo?.conflicted ?? 0,
        repositoryError: Boolean(repositoryError.value),
        views,
        selectedViewId: selectedView.value,
        commandGroupLabel: 'Project commands',
        commands: commandDefinitions.value.map((command) => {
          const latest = commandRunFor(command.id);
          return {
            id: command.id,
            label: command.title,
            color: command.color ?? 'transparent',
            icon: commandIcon(command),
            kind: command.kind,
            group: command.group,
            running: latest?.state === 'running',
            lastRun: latest
              ? `${latest.state}${latest.exit_code === undefined ? '' : ` (exit ${latest.exit_code})`}`
              : undefined,
          };
        }),
        commandGroupExpanded: commandGroupExpanded.value,
        collapsedCommandGroups: commandGroupsCollapsed.value[current.id],
        driveRunning: drive.running,
        driveTool: tool,
        driveToolLabel: aiToolLabel(tool),
        driveDisabled: drive.disabled,
        driveDisabledReason: drive.disabledReason,
        driveOptionsOpen: driveOptionsOpen.value,
        driveOptionsAnchor: driveOptionsAnchor.value,
        driveTools: aiTools.value,
        driveToolsLoading: aiSettingsLoading.value,
        driveToolsError: aiSettingsMessage.value,
        driveSelection: driveOverridesByProject.value[current.id] ?? {},
        driveDefaultSelection: aiDefaults.value,
        conversationOpen:
          conversationOpen.value &&
          conversationConnectionId.value === projectChatConnectionId(current.id, aiDefaults.value.tool),
        conversationDisabled: drivePendingByProject.value[current.id],
        openCount: counts.open,
        upNextCount: counts.up_next,
        activeCount: counts.active,
        countsPartial,
        collapseControl: true,
      },
    };
  }
  return { sidebarSurfaceProps };
}
