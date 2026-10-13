import './settings-workspace.css';
import '@awesome.me/webawesome/dist/components/checkbox/checkbox.js';

import { rem } from '@kerfjs/ui/css-values';
import { Grid } from '@kerfjs/ui/grid';
import { List } from '@kerfjs/ui/list';
import { Select } from '@kerfjs/ui/select';
import { Text } from '@kerfjs/ui/text';

import type { AiToolDefaults, AiToolDescriptor } from '../api';
import { COMMANDS_AND_AI_ACTIONS } from '../interaction-attrs/commands-and-ai';
import { NOTIFICATIONS_AND_LINKS_ACTIONS } from '../interaction-attrs/notifications-and-links';
import { formatPermissionDelay, type PermissionAutomation, permissionDelaysFor } from '../permission-notifications';
import { AiToolSettings } from './ai-tool-settings';
import type { CommandSettingsEditorProps } from './command-settings-editor';
import { CommandSettingsEditor } from './command-settings-editor';
import { KeyboardSettings, type KeyboardSettingsProps } from './keyboard-settings';
import { type SettingsCategory, settingsCategoryTitle } from './settings-navigation';
import {
  AccountsSettings,
  type AccountsSettingsProps,
  TicketSourcesSettings,
  type TicketSourcesSettingsProps,
} from './ticket-sources-settings';
import { TrashSettings } from './trash-settings';

export interface SettingsWorkspaceProps {
  category: SettingsCategory;
  sources: TicketSourcesSettingsProps;
  accounts: AccountsSettingsProps;
  ai: { tools: readonly AiToolDescriptor[]; selection: AiToolDefaults; loading: boolean; message: string };
  commands: CommandSettingsEditorProps;
  lifecycle: { days: number; message: string };
  terminals: { inheritGlobalShellHistory: boolean; message: string };
  permissions: { automation: PermissionAutomation; delays: readonly number[] };
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
  permissions,
  columns,
  general,
  keyboard,
}: SettingsWorkspaceProps) {
  return (
    <section
      class="settings-workspace"
      data-component="settings-workspace"
      aria-label={`${settingsCategoryTitle(category)} settings`}
      data-settings-category={category}
    >
      {category === 'sources' && <TicketSourcesSettings {...sources} />}
      {category === 'accounts' && <AccountsSettings {...accounts} />}
      {category === 'ai' && (
        <AiToolSettings tools={ai.tools} selection={ai.selection} loading={ai.loading} message={ai.message} />
      )}
      {category === 'commands' && <CommandSettingsEditor {...commands} />}
      {category === 'lifecycle' && <TrashSettings days={lifecycle.days} message={lifecycle.message} />}
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
              label="Automatic decision"
              value={permissions.automation.action}
              choices={[
                { value: 'off', label: 'Off' },
                { value: 'allow', label: 'Auto-allow' },
                { value: 'deny', label: 'Auto-deny' },
              ]}
            />
            <Select
              name="permission-automation-delay"
              label="After visible for"
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
    </section>
  );
}
