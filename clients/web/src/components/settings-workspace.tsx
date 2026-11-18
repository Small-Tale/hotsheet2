import './settings-workspace.css';

import { Checkbox } from '@kerfjs/ui/checkbox';
import { rem } from '@kerfjs/ui/css-values';
import { Grid } from '@kerfjs/ui/grid';
import { List } from '@kerfjs/ui/list';
import { Select } from '@kerfjs/ui/select';
import { Text } from '@kerfjs/ui/text';
import type { SafeHtml } from 'kerfjs';

import type { AiToolDefaults, AiToolDescriptor } from '../api';
import { COMMANDS_AND_AI_ACTIONS } from '../interaction-attrs/commands-and-ai';
import { NOTIFICATIONS_AND_LINKS_ACTIONS } from '../interaction-attrs/notifications-and-links';
import {
  formatPermissionDelay,
  type PermissionAutomation,
  permissionDelaysFor,
  type PermissionRule,
} from '../permission-notifications';
import type { ThemePreference } from '../theme-preference';
import { AiToolSettings } from './ai-tool-settings';
import type { CommandSettingsEditorProps } from './command-settings-editor';
import { CommandSettingsEditor } from './command-settings-editor';
import { DevelopmentStrategySettings, type DevelopmentStrategySettingsProps } from './development-strategy-settings';
import { GuidanceSettings, type GuidanceSettingsProps } from './guidance-settings';
import { HostDiskSettings, type HostDiskSettingsProps } from './host-disk-settings';
import { KeyboardSettings, type KeyboardSettingsProps } from './keyboard-settings';
import { NotificationSettings, type NotificationSettingsProps } from './notification-settings';
import { OrchestrationSettings, type OrchestrationSettingsProps } from './orchestration-settings';
import { PermissionRulesSettings, type PermissionRulesSettingsProps } from './permission-rules-settings';
import { ProjectGeneralSettings, type ProjectGeneralSettingsProps } from './project-general-settings';
import { RegisteredProjectsSettings, type RegisteredProjectsSettingsProps } from './registered-projects-settings';
import { RemoteDevicesSettings, type RemoteDevicesSettingsProps } from './remote-devices-settings';
import { type SettingsCategory, settingsCategoryTitle } from './settings-navigation';
import { StandbySettings, type StandbySettingsProps } from './standby-settings';
import { TerminalMemorySettings, type TerminalMemorySettingsProps } from './terminal-memory-settings';
import { ThemePicker } from './theme-picker';
import { TicketGroupSettings, type TicketGroupSettingsProps } from './ticket-group-settings';
import {
  AccountsSettings,
  type AccountsSettingsProps,
  TicketSourcesSettings,
  type TicketSourcesSettingsProps,
} from './ticket-sources-settings';
import { type TrashRetentionDraft, TrashSettings } from './trash-settings';

export interface SettingsWorkspaceProps {
  category: SettingsCategory;
  /** Project Settings → General: Project Type (HS2-W27N1D). */
  projectGeneral?: ProjectGeneralSettingsProps;
  /** Project Settings → Orchestration, for an Orchestration project (HS2-W27N1D). */
  orchestration?: OrchestrationSettingsProps;
  sources: TicketSourcesSettingsProps;
  /** Project Settings › Ticket groups (HS2-GXKX1Q). */
  groups?: TicketGroupSettingsProps;
  accounts: AccountsSettingsProps;
  ai: { tools: readonly AiToolDescriptor[]; selection: AiToolDefaults; loading: boolean; message: string };
  guidance?: GuidanceSettingsProps;
  commands: CommandSettingsEditorProps;
  lifecycle: { days: number; message: string; draft?: TrashRetentionDraft };
  terminals: { inheritGlobalShellHistory: boolean; message: string };
  /** App Settings → Terminal memory (HS2-Y5DYC0). */
  terminalMemory?: TerminalMemorySettingsProps;
  /** App Settings → Notifications (HS2-DPBJJA). */
  notifications?: NotificationSettingsProps;
  /** App Settings → Devices (HS2-DXM0YN). */
  devices?: RemoteDevicesSettingsProps;
  /** App Settings → Projects (HS2-R806FZ). */
  projects?: RegisteredProjectsSettingsProps;
  permissions: {
    automation: PermissionAutomation;
    delays: readonly number[];
    owner?: PermissionRulesSettingsProps['owner'];
    /** Remembered answers for this project (HS2-JNHAYB). */
    rules?: readonly PermissionRule[];
    editor?: PermissionRulesSettingsProps['editor'];
    dialog?: PermissionRulesSettingsProps['dialog'];
    preview?: PermissionRulesSettingsProps['preview'];
    revoking?: readonly string[];
    rulesMessage?: string;
    locked?: boolean;
    rulesRefreshFailed?: boolean;
    legacy?: PermissionRulesSettingsProps['legacy'];
    /**
     * The pending permission request's popup, laid out in flow above the automation controls while this
     * category shows, instead of floating over them (HS2-84XE8Q).
     */
    pendingRequest?: SafeHtml;
  };
  /** Project Settings › General (HS2-FN018Q): automatic grouping and the board's Verified column. */
  columns: { hideVerified: boolean; autoGroupNewTickets?: boolean; autoGroupMessage?: string };
  developmentStrategy?: DevelopmentStrategySettingsProps;
  /** App Settings → General: the browser's theme (HS2-EFTHS6); `standby` is the machine-wide standby
   *  section (HS2-EQVZTZ); `hostDisks` the server host's monitored disks (HS2-KBVZV1). */
  general: {
    showLoadingActivity: boolean;
    theme: ThemePreference;
    standby?: StandbySettingsProps;
    hostDisks?: HostDiskSettingsProps;
  };
  keyboard: KeyboardSettingsProps;
}

export function SettingsWorkspace({
  category,
  projectGeneral = {},
  orchestration = {},
  sources,
  groups = {},
  accounts,
  ai,
  guidance,
  commands,
  lifecycle,
  terminals,
  terminalMemory = {},
  notifications = {},
  devices = {},
  projects = {},
  permissions,
  columns,
  developmentStrategy = {},
  general,
  keyboard,
}: SettingsWorkspaceProps) {
  return (
    <section
      class="settings-workspace"
      data-component="settings-workspace"
      aria-label={`${settingsCategoryTitle(category)} Settings`}
      data-settings-category={category}
    >
      {category === 'guidance' && guidance ? (
        <GuidanceSettings {...guidance} />
      ) : (
        <List contentWidth="wide" inset="document">
          {category === 'project-general' && <ProjectGeneralSettings {...projectGeneral} />}
          {category === 'orchestration' && <OrchestrationSettings {...orchestration} />}
          {category === 'sources' && <TicketSourcesSettings {...sources} />}
          {category === 'groups' && <TicketGroupSettings {...groups} />}
          {category === 'accounts' && <AccountsSettings {...accounts} />}
          {category === 'terminal-memory' && <TerminalMemorySettings {...terminalMemory} />}
          {category === 'notifications' && <NotificationSettings {...notifications} />}
          {category === 'devices' && <RemoteDevicesSettings {...devices} />}
          {category === 'projects' && <RegisteredProjectsSettings {...projects} />}
          {category === 'ai' && (
            <AiToolSettings tools={ai.tools} selection={ai.selection} loading={ai.loading} message={ai.message} />
          )}
          {category === 'commands' && <CommandSettingsEditor {...commands} />}
          {category === 'development-strategy' && <DevelopmentStrategySettings {...developmentStrategy} />}
          {category === 'lifecycle' && (
            <TrashSettings days={lifecycle.days} message={lifecycle.message} draft={lifecycle.draft} />
          )}
          {category === 'terminals' && (
            <List gap="xs">
              <Checkbox
                action={COMMANDS_AND_AI_ACTIONS.toggleGlobalShellHistory.value}
                checked={terminals.inheritGlobalShellHistory}
                hint="By default, each terminal keeps private, machine-local command recall for this project and terminal. This opt-out applies only on this machine and affects newly created terminals."
                label="Use my global shell history"
              />
              <Text tone="quiet" size="compact" role="status" flush>
                {terminals.message}
              </Text>
            </List>
          )}
          {category === 'permissions' && (
            // A pending request sits above the form as its own region (24px), keyed so its arrival or
            // dismissal never re-creates the form's controls (HS2-84XE8Q).
            <List gap="l">
              {permissions.pendingRequest && (
                <div data-key="permission-pending-request">{permissions.pendingRequest}</div>
              )}
              <List gap="xs" rootAttributes={{ 'data-key': 'permission-automation' }}>
                <Grid minColumnWidth={rem(15)} gap="m">
                  <Select
                    name="permission-automation-action"
                    label="Automatic Decision"
                    value={permissions.automation.action}
                    choices={[
                      { value: 'off', label: 'Off' },
                      { value: 'allow', label: 'Auto-Allow' },
                      { value: 'deny', label: 'Auto-Deny' },
                    ]}
                  />
                  <Select
                    name="permission-automation-delay"
                    label="After Visible For"
                    value={String(permissions.automation.delayMs)}
                    disabled={permissions.automation.action === 'off'}
                    choices={permissions.delays
                      .filter((value) => permissionDelaysFor(permissions.automation.action).includes(value))
                      .map((value) => ({ value: String(value), label: formatPermissionDelay(value) }))}
                  />
                </Grid>
                <Text tone="quiet" size="compact" data-permission-note="true">
                  Runs only while this project's permission popup is visible. Ignore pauses the timer; Stop auto-allow
                  or Stop auto-deny disables that automatic decision for this request. Auto-allow after 0 seconds allows
                  each request without showing the popup; it is still recorded in Notifications.
                </Text>
                <PermissionRulesSettings
                  rules={permissions.rules ?? []}
                  owner={permissions.owner}
                  editor={permissions.editor}
                  dialog={permissions.dialog}
                  preview={permissions.preview}
                  revoking={permissions.revoking}
                  message={permissions.rulesMessage}
                  legacy={permissions.legacy}
                  locked={permissions.locked}
                  refreshFailed={permissions.rulesRefreshFailed}
                />
              </List>
            </List>
          )}
          {category === 'project-general' && (
            <List gap="m">
              <Checkbox
                action={NOTIFICATIONS_AND_LINKS_ACTIONS.toggleAutoGroupNewTickets.value}
                checked={columns.autoGroupNewTickets ?? true}
                hint="Each new ticket, however it is created, is offered to this project's default AI tool, which picks a group or names a new one. This uses one AI turn per batch of new tickets."
                label="Group new tickets automatically"
              />
              {columns.autoGroupMessage ? (
                <Text tone="danger" role="alert">
                  {columns.autoGroupMessage}
                </Text>
              ) : null}
              <Checkbox
                action={NOTIFICATIONS_AND_LINKS_ACTIONS.toggleVerifiedColumn.value}
                checked={columns.hideVerified}
                hint="Verified tickets appear in Completed."
                label="Hide Verified column"
              />
            </List>
          )}
          {category === 'general' && (
            <>
              <ThemePicker value={general.theme} />
              <Checkbox
                action={COMMANDS_AND_AI_ACTIONS.toggleLoadingActivity.value}
                checked={general.showLoadingActivity}
                hint="Shows a small label at the top of the app describing what the server is doing (for example “Loading tickets”). This applies only on this machine."
                label="Show loading activity"
              />
              {general.standby && <StandbySettings {...general.standby} />}
              {general.hostDisks && <HostDiskSettings {...general.hostDisks} />}
            </>
          )}
          {category === 'keyboard' && <KeyboardSettings {...keyboard} />}
        </List>
      )}
    </section>
  );
}
