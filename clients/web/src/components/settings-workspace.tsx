import '@awesome.me/webawesome/dist/components/checkbox/checkbox.js';

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
import { KeyboardSettings, type KeyboardSettingsProps } from './keyboard-settings';
import { PermissionRulesSettings } from './permission-rules-settings';
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
  commands: CommandSettingsEditorProps;
  lifecycle: { days: number; message: string; draft?: TrashRetentionDraft };
  terminals: { inheritGlobalShellHistory: boolean; message: string };
  /** App Settings → Terminal memory (HS2-Y5DYC0). */
  terminalMemory?: TerminalMemorySettingsProps;
  permissions: {
    automation: PermissionAutomation;
    delays: readonly number[];
    /** Remembered answers for this project (HS2-JNHAYB). */
    rules?: readonly PermissionRule[];
    dialog?: { query: string; page: number };
    revoking?: readonly string[];
    rulesMessage?: string;
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
            <wa-checkbox
              {...COMMANDS_AND_AI_ACTIONS.toggleGlobalShellHistory.attrs}
              checked={terminals.inheritGlobalShellHistory}
              hint="By default, each terminal keeps private, machine-local command recall for this project and terminal. This opt-out applies only on this machine and affects newly created terminals."
            >
              Use my global shell history
            </wa-checkbox>
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
              auto-allow or Stop auto-deny disables that automatic decision for this request. Auto-allow after 0 seconds
              allows each request without showing the popup; it is still recorded in Notifications.
            </Text>
            <PermissionRulesSettings
              rules={permissions.rules ?? []}
              dialog={permissions.dialog}
              revoking={permissions.revoking}
              message={permissions.rulesMessage}
            />
          </List>
        )}
        {category === 'columns' && (
          <wa-checkbox
            {...NOTIFICATIONS_AND_LINKS_ACTIONS.toggleVerifiedColumn.attrs}
            checked={columns.hideVerified}
            hint="Verified tickets appear in Completed."
          >
            Hide Verified column
          </wa-checkbox>
        )}
        {category === 'general' && (
          <wa-checkbox
            {...COMMANDS_AND_AI_ACTIONS.toggleLoadingActivity.attrs}
            checked={general.showLoadingActivity}
            hint="Shows a small label at the top of the app describing what the server is doing (for example “Loading tickets”). This applies only on this machine."
          >
            Show loading activity
          </wa-checkbox>
        )}
        {category === 'keyboard' && <KeyboardSettings {...keyboard} />}
      </List>
    </section>
  );
}
