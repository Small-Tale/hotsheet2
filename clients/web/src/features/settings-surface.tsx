import type { ProviderConnection } from '../api';
import type { SettingsCategory } from '../components/settings-navigation';
import { SettingsWorkspace, type SettingsWorkspaceProps } from '../components/settings-workspace';
import type { TrashRetentionDraft } from '../components/trash-settings';
import type { Project } from '../interactions/types';
import type { ProjectTicketSources } from '../new-ticket-source';
import { PERMISSION_DELAYS } from '../permission-notifications';

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
  commandSettingsDefinitions: (projectId: string) => Props['commands']['commands'];
  commandSettingsExtraGroups: (projectId: string) => Props['commands']['extraGroups'];
  commandSettingsEditingId: Readable<Props['commands']['editingId']>;
  commandSelection: (projectId: string) => Props['commands']['selectedIds'];
  commandIconSearch: Readable<Props['commands']['iconSearch']>;
  commandSettingsMessage: (projectId: string) => Props['commands']['message'];
  trashCleanupDaysByProject: Readable<Record<string, number>>;
  trashSettingsMessagesByProject: Readable<Record<string, string>>;
  trashRetentionDraftsByProject: Readable<Record<string, TrashRetentionDraft>>;
  inheritGlobalShellHistory: Readable<boolean>;
  terminalSettingsMessage: Readable<string>;
  permissionAutomation: (projectId: string) => Props['permissions']['automation'];
  permissionRules: (projectId: string) => Props['permissions']['rules'];
  permissionRulesMessage: Readable<string>;
  revokingPermissionRules: Readable<readonly string[]>;
  hideVerifiedColumn: () => boolean;
  showLoadingActivity: Readable<boolean>;
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
    commandSettingsDefinitions,
    commandSettingsExtraGroups,
    commandSettingsEditingId,
    commandSelection,
    commandIconSearch,
    commandSettingsMessage,
    trashCleanupDaysByProject,
    trashSettingsMessagesByProject,
    trashRetentionDraftsByProject,
    inheritGlobalShellHistory,
    terminalSettingsMessage,
    permissionAutomation,
    permissionRules,
    permissionRulesMessage,
    revokingPermissionRules,
    hideVerifiedColumn,
    showLoadingActivity,
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
        commands={{
          commands: commandSettingsDefinitions(current.id),
          extraGroups: commandSettingsExtraGroups(current.id),
          editingId: commandSettingsEditingId.value,
          selectedIds: commandSelection(current.id),
          iconSearch: commandIconSearch.value,
          message: commandSettingsMessage(current.id),
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
        permissions={{
          automation: permissionAutomation(current.id),
          delays: PERMISSION_DELAYS,
          // Loaded only while the Permissions category shows (HS2-JNHAYB).
          rules: settingsCategory() === 'permissions' ? permissionRules(current.id) : [],
          revoking: revokingPermissionRules.value,
          rulesMessage: permissionRulesMessage.value,
        }}
        columns={{ hideVerified: hideVerifiedColumn() }}
        general={{ showLoadingActivity: showLoadingActivity.value }}
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
