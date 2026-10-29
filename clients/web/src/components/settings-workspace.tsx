import './settings-workspace.css';

import { Checkbox } from '@kerfjs/ui/checkbox';
import { rem } from '@kerfjs/ui/css-values';
import { Grid } from '@kerfjs/ui/grid';
import { List } from '@kerfjs/ui/list';
import { Select } from '@kerfjs/ui/select';
import { Text } from '@kerfjs/ui/text';

import type { AiToolDefaults, AiToolDescriptor } from '../api';
import { COMMANDS_AND_AI_ACTIONS } from '../interaction-attrs/commands-and-ai';
import { NOTIFICATIONS_AND_LINKS_ACTIONS } from '../interaction-attrs/notifications-and-links';
import {
  formatPermissionDelay,
  type PermissionAutomation,
  permissionDelaysFor,
  type PermissionRule,
} from '../permission-notifications';
import { AiToolSettings } from './ai-tool-settings';
import type { CommandSettingsEditorProps } from './command-settings-editor';
import { CommandSettingsEditor } from './command-settings-editor';
import { GuidanceSettings, type GuidanceSettingsProps } from './guidance-settings';
import { KeyboardSettings, type KeyboardSettingsProps } from './keyboard-settings';
import { PermissionRulesSettings, type PermissionRulesSettingsProps } from './permission-rules-settings';
import { type SettingsCategory, settingsCategoryTitle } from './settings-navigation';
import { TerminalMemorySettings, type TerminalMemorySettingsProps } from './terminal-memory-settings';
import {
  AccountsSettings,
  type AccountsSettingsProps,
  TicketSourcesSettings,
  type TicketSourcesSettingsProps,
} from './ticket-sources-settings';
import { type TrashRetentionDraft, TrashSettings } from './trash-settings';

export interface SettingsWorkspaceProps {
  category: SettingsCategory;
  sources: TicketSourcesSettingsProps;
  accounts: AccountsSettingsProps;
  ai: { tools: readonly AiToolDescriptor[]; selection: AiToolDefaults; loading: boolean; message: string };
  guidance?: GuidanceSettingsProps;
  commands: CommandSettingsEditorProps;
  lifecycle: { days: number; message: string; draft?: TrashRetentionDraft };
  terminals: { inheritGlobalShellHistory: boolean; message: string };
  /** App Settings → Terminal memory (HS2-Y5DYC0). */
  terminalMemory?: TerminalMemorySettingsProps;
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
  };
  columns: { hideVerified: boolean };
  general: { showLoadingActivity: boolean };
  keyboard: KeyboardSettingsProps;
}

export function SettingsWorkspace({
  category,
  sources,
  accounts,
  ai,
  guidance,
  commands,
  lifecycle,
  terminals,
  terminalMemory = {},
  permissions,
  columns,
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
          {category === 'sources' && <TicketSourcesSettings {...sources} />}
          {category === 'accounts' && <AccountsSettings {...accounts} />}
          {category === 'terminal-memory' && <TerminalMemorySettings {...terminalMemory} />}
          {category === 'ai' && (
            <AiToolSettings tools={ai.tools} selection={ai.selection} loading={ai.loading} message={ai.message} />
          )}
          {category === 'commands' && <CommandSettingsEditor {...commands} />}
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
            <List gap="xs">
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
                Runs only while this project's floating permission popup is visible. Ignore pauses the timer; Stop
                auto-allow or Stop auto-deny disables that automatic decision for this request. Auto-allow after 0
                seconds allows each request without showing the popup; it is still recorded in Notifications.
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
          )}
          {category === 'columns' && (
            <Checkbox
              action={NOTIFICATIONS_AND_LINKS_ACTIONS.toggleVerifiedColumn.value}
              checked={columns.hideVerified}
              hint="Verified tickets appear in Completed."
              label="Hide Verified column"
            />
          )}
          {category === 'general' && (
            <Checkbox
              action={COMMANDS_AND_AI_ACTIONS.toggleLoadingActivity.value}
              checked={general.showLoadingActivity}
              hint="Shows a small label at the top of the app describing what the server is doing (for example “Loading tickets”). This applies only on this machine."
              label="Show loading activity"
            />
          )}
          {category === 'keyboard' && <KeyboardSettings {...keyboard} />}
        </List>
      )}
    </section>
  );
}
