import type { ProviderConnection, TerminalLimits } from '../api';
import type { SettingsCategory } from '../components/settings-navigation';
import { SettingsWorkspace, type SettingsWorkspaceProps } from '../components/settings-workspace';
import type { TrashRetentionDraft } from '../components/trash-settings';
import type { Project } from '../interactions/types';
import type { ProjectTicketSources } from '../new-ticket-source';
import { PERMISSION_DELAYS } from '../permission-notifications';
import type { ThemePreference } from '../theme-preference';
import { ticketGroupOperation, ticketGroupSettingsByProject } from '../ticket-group-state';

/** A signal as the settings projection reads it. */
interface Readable<T> {
  readonly value: T;
}
type Props = SettingsWorkspaceProps;

/** Live application bindings the Settings workspace projection reads (HS2-BSSWGV). */
export interface SettingsSurfaceDependencies {
  settingsCategory: () => SettingsCategory;
  defaultProviders: Readable<Record<string, ProjectTicketSources | undefined>>;
  providerConnections: Readable<readonly ProviderConnection[]>;
  providerSettingsError: Readable<Props['sources']['error']>;
  ticketSourceSetupProject: Readable<unknown>;
  providerAccounts: Readable<Props['accounts']['accounts']>;
  providerAccountsError: Readable<Props['accounts']['error']>;
  signingOutAccount: Readable<Props['accounts']['signingOut']>;
  unusedAccountSourceChoice: Readable<Props['accounts']['sourceRemovalChoice']>;
  removingUnusedAccountSource: Readable<Props['accounts']['removingSource']>;
  aiTools: Readable<Props['ai']['tools']>;
  aiDefaults: Readable<Props['ai']['selection']>;
  aiSettingsLoading: Readable<boolean>;
  aiSettingsMessage: Readable<string>;
  guidance: () => Props['guidance'];
  commandSettingsDefinitions: (projectId: string) => Props['commands']['commands'];
  commandSettingsExtraGroups: (projectId: string) => Props['commands']['extraGroups'];
  commandSettingsEditingId: Readable<Props['commands']['editingId']>;
  commandSelection: (projectId: string) => Props['commands']['selectedIds'];
  commandIconSearch: Readable<Props['commands']['iconSearch']>;
  commandSettingsMessage: (projectId: string) => Props['commands']['message'];
  /** The editor's first-load state and failure reason (HS2-BZKTDC). */
  commandSettingsLoadState: (projectId: string) => Props['commands']['loadState'];
  commandSettingsLoadError: (projectId: string) => Props['commands']['loadError'];
  trashCleanupDaysByProject: Readable<Record<string, number>>;
  trashSettingsMessagesByProject: Readable<Record<string, string>>;
  /** Project Settings › General: automatic grouping of new tickets (HS2-FN018Q). */
  autoGroupNewTicketsByProject: Readable<Record<string, boolean>>;
  autoGroupMessagesByProject: Readable<Record<string, string>>;
  trashRetentionDraftsByProject: Readable<Record<string, TrashRetentionDraft>>;
  inheritGlobalShellHistory: Readable<boolean>;
  terminalSettingsMessage: Readable<string>;
  /** Machine-wide terminal limits (HS2-Y5DYC0). */
  terminalLimits?: Readable<TerminalLimits | undefined>;
  terminalLimitsMessage?: Readable<string>;
  terminalLimitDeviceDefaults?: () => NonNullable<Props['terminalMemory']>['deviceDefaults'];
  /** App Settings → Notifications (HS2-DPBJJA). */
  notificationSettings?: () => Props['notifications'];
  /** App Settings → Devices (HS2-DXM0YN). */
  remoteDevices?: () => Props['devices'];
  /** App Settings → Projects (HS2-R806FZ). */
  registeredProjects?: () => Props['projects'];
  developmentStrategy?: (projectId: string) => Props['developmentStrategy'];
  /** Project Settings → General: Project Type (HS2-W27N1D). */
  projectGeneral?: (projectId: string) => Props['projectGeneral'];
  /** Project Settings → Orchestration (HS2-W27N1D). */
  orchestration?: (current: Project) => Props['orchestration'];
  permissionAutomation: (projectId: string) => Props['permissions']['automation'];
  permissionRules: (projectId: string) => Props['permissions']['rules'];
  permissionRuleEditorFor: (projectId: string, visible: boolean) => Props['permissions']['editor'];
  permissionRulesDialogFor: (projectId: string, visible: boolean) => Props['permissions']['dialog'];
  permissionRulesPreviewFor: (projectId: string, visible: boolean) => Props['permissions']['preview'];
  permissionRulesMessage: Readable<string>;
  lockedPermissionRules: Readable<ReadonlySet<string>>;
  unrefreshedPermissionRules: Readable<ReadonlySet<string>>;
  permissionLegacyDiscardFor: (projectId: string) => NonNullable<Props['permissions']['legacy']>['review'];
  /** The pending request's in-flow popup while Settings → Permissions hosts it (HS2-84XE8Q). */
  pendingPermissionRequest?: () => Props['permissions']['pendingRequest'];
  revokingPermissionRules: Readable<readonly string[]>;
  hideVerifiedColumn: () => boolean;
  showLoadingActivity: Readable<boolean>;
  /** App Settings → General → Theme (HS2-EFTHS6). */
  themePreference: Readable<ThemePreference>;
  /** App Settings → General → Standby (HS2-EQVZTZ). */
  standbySettings?: () => NonNullable<Props['general']['standby']>;
  /** App Settings → General → Host health disks (HS2-KBVZV1). */
  hostDiskSettings?: () => NonNullable<Props['general']['hostDisks']>;
  /** App Settings › Features (HS2-3GJZGG). */
  featureSettings?: () => NonNullable<Props['features']>;
  keyboardShortcutOverrides: Readable<Props['keyboard']['overrides']>;
  capturingShortcutId: Readable<Props['keyboard']['capturingId']>;
  appleShortcutPlatform: boolean;
}

/**
 * Projects the selected project's Settings workspace: ticket sources (with each shared connection's
 * other projects), accounts, AI defaults, commands, lifecycle, terminals, permissions, columns,
 * general, and keyboard settings. Extracted from the application runtime without behavior changes
 * (HS2-BSSWGV).
 */
export function createSettingsSurface(dependencies: SettingsSurfaceDependencies) {
  const {
    settingsCategory,
    defaultProviders,
    providerConnections,
    providerSettingsError,
    ticketSourceSetupProject,
    providerAccounts,
    providerAccountsError,
    signingOutAccount,
    unusedAccountSourceChoice,
    removingUnusedAccountSource,
    aiTools,
    aiDefaults,
    aiSettingsLoading,
    aiSettingsMessage,
    guidance,
    commandSettingsDefinitions,
    commandSettingsExtraGroups,
    commandSettingsEditingId,
    commandSelection,
    commandIconSearch,
    commandSettingsMessage,
    commandSettingsLoadState,
    commandSettingsLoadError,
    trashCleanupDaysByProject,
    trashSettingsMessagesByProject,
    autoGroupNewTicketsByProject,
    autoGroupMessagesByProject,
    trashRetentionDraftsByProject,
    inheritGlobalShellHistory,
    terminalSettingsMessage,
    terminalLimits,
    terminalLimitsMessage,
    terminalLimitDeviceDefaults,
    notificationSettings,
    remoteDevices,
    registeredProjects,
    developmentStrategy,
    projectGeneral,
    orchestration,
    permissionAutomation,
    permissionRules,
    permissionRulesDialogFor,
    permissionRuleEditorFor,
    permissionRulesPreviewFor,
    permissionRulesMessage,
    lockedPermissionRules,
    unrefreshedPermissionRules,
    permissionLegacyDiscardFor,
    pendingPermissionRequest,
    revokingPermissionRules,
    hideVerifiedColumn,
    showLoadingActivity,
    themePreference,
    standbySettings,
    featureSettings,
    hostDiskSettings,
    keyboardShortcutOverrides,
    capturingShortcutId,
    appleShortcutPlatform,
  } = dependencies;
  function projectSourcesSettings(current: Project) {
    const sources = defaultProviders.value[current.id]?.sources ?? [];
    return {
      sources: sources.map((source) => {
        const connection = providerConnections.value.find((item) => item.id === source.connectionId);
        return {
          connectionId: source.connectionId,
          name: source.name,
          provider: source.provider,
          locator: source.locator,
          default: source.default,
          color: source.color,
          disabled: Boolean(connection?.disabled),
          unverifiedRecovery: source.unverifiedRecovery,
          identityReviewRequired: source.identityReviewRequired,
          identityMismatch: source.identityMismatch,
          storeUnavailable: source.storeUnavailable,
          ticketPrefix: source.ticketPrefix,
          sharedWith: (connection?.projects ?? [])
            .filter((project) => project.id !== current.id)
            .map((project) => project.alias),
        };
      }),
      error: providerSettingsError.value,
      setupOpen: Boolean(ticketSourceSetupProject.value),
    };
  }
  function settingsWorkspace(current: Project) {
    return (
      <SettingsWorkspace
        category={settingsCategory()}
        developmentStrategy={developmentStrategy?.(current.id)}
        projectGeneral={projectGeneral?.(current.id)}
        orchestration={settingsCategory() === 'orchestration' ? orchestration?.(current) : undefined}
        sources={projectSourcesSettings(current)}
        accounts={{
          accounts: providerAccounts.value,
          error: providerAccountsError.value,
          signingOut: signingOutAccount.value,
          sourceRemovalChoice: unusedAccountSourceChoice.value,
          removingSource: removingUnusedAccountSource.value,
        }}
        ai={{
          tools: aiTools.value,
          selection: aiDefaults.value,
          loading: aiSettingsLoading.value,
          message: aiSettingsMessage.value,
        }}
        guidance={guidance()}
        groups={{
          groups: ticketGroupSettingsByProject.value[current.id]?.groups,
          error: ticketGroupSettingsByProject.value[current.id]?.error,
          busyGroup:
            ticketGroupOperation.value?.projectId === current.id ? ticketGroupOperation.value.groupId : undefined,
        }}
        commands={{
          commands: commandSettingsDefinitions(current.id),
          extraGroups: commandSettingsExtraGroups(current.id),
          editingId: commandSettingsEditingId.value,
          selectedIds: commandSelection(current.id),
          iconSearch: commandIconSearch.value,
          message: commandSettingsMessage(current.id),
          loadState: commandSettingsLoadState(current.id),
          loadError: commandSettingsLoadError(current.id),
          aiTools: aiTools.value,
          aiDefaults: aiDefaults.value,
        }}
        lifecycle={{
          days: trashCleanupDaysByProject.value[current.id] ?? 30,
          message: trashSettingsMessagesByProject.value[current.id] ?? '',
          draft: trashRetentionDraftsByProject.value[current.id],
        }}
        terminals={{
          inheritGlobalShellHistory: inheritGlobalShellHistory.value,
          message: terminalSettingsMessage.value,
        }}
        notifications={notificationSettings?.()}
        devices={remoteDevices?.()}
        projects={registeredProjects?.()}
        terminalMemory={{
          limits: terminalLimits?.value,
          deviceDefaults: terminalLimitDeviceDefaults?.(),
          message: terminalLimitsMessage?.value,
        }}
        permissions={{
          automation: permissionAutomation(current.id),
          delays: PERMISSION_DELAYS,
          owner: { id: current.id, name: current.name, root: current.root },
          // Loaded only while the Permissions category shows (HS2-JNHAYB).
          rules: settingsCategory() === 'permissions' ? permissionRules(current.id) : [],
          editor: permissionRuleEditorFor(current.id, settingsCategory() === 'permissions'),
          dialog: permissionRulesDialogFor(current.id, settingsCategory() === 'permissions'),
          preview: permissionRulesPreviewFor(current.id, settingsCategory() === 'permissions'),
          revoking: revokingPermissionRules.value,
          rulesMessage: permissionRulesMessage.value,
          locked: lockedPermissionRules.value.has(current.id),
          rulesRefreshFailed: unrefreshedPermissionRules.value.has(current.id),
          pendingRequest: settingsCategory() === 'permissions' ? pendingPermissionRequest?.() : undefined,
          legacy: {
            sources: projectSourcesSettings(current)
              .sources.filter(
                (source) => source.provider === 'git' && !source.unverifiedRecovery && !source.identityMismatch,
              )
              .map((source) => ({ id: source.connectionId, name: source.name, locator: source.locator })),
            review: permissionLegacyDiscardFor(current.id),
          },
        }}
        columns={{
          hideVerified: hideVerifiedColumn(),
          autoGroupNewTickets: autoGroupNewTicketsByProject.value[current.id] ?? true,
          autoGroupMessage: autoGroupMessagesByProject.value[current.id] ?? '',
        }}
        general={{
          showLoadingActivity: showLoadingActivity.value,
          theme: themePreference.value,
          standby: standbySettings?.(),
          hostDisks: hostDiskSettings?.(),
        }}
        features={featureSettings?.()}
        keyboard={{
          overrides: keyboardShortcutOverrides.value,
          capturingId: capturingShortcutId.value,
          apple: appleShortcutPlatform,
        }}
      />
    );
  }
  return { projectSourcesSettings, settingsWorkspace };
}
