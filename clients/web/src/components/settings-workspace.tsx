import './settings-workspace.css';

import { Select } from '@kerfjs/ui/select';

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
  ConnectionsSettings,
  type ConnectionsSettingsProps,
  TicketSourcesSettings,
  type TicketSourcesSettingsProps,
} from './ticket-sources-settings';
import { TrashSettings } from './trash-settings';

export interface SettingsWorkspaceProps {
  category: SettingsCategory;
  sources: TicketSourcesSettingsProps;
  connections: ConnectionsSettingsProps;
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
  connections,
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
      class="project-settings"
      data-component="settings-workspace"
      aria-label={`${settingsCategoryTitle(category)} settings`}
      data-settings-category={category}
    >
      {category === 'sources' && <TicketSourcesSettings {...sources} />}
      {category === 'connections' && <ConnectionsSettings {...connections} />}
      {category === 'ai' && (
        <AiToolSettings tools={ai.tools} selection={ai.selection} loading={ai.loading} message={ai.message} />
      )}
      {category === 'commands' && <CommandSettingsEditor {...commands} />}
      {category === 'lifecycle' && <TrashSettings days={lifecycle.days} message={lifecycle.message} />}
      {category === 'terminals' && (
        <>
          <label class="project-settings__option">
            <input
              type="checkbox"
              {...COMMANDS_AND_AI_ACTIONS.toggleGlobalShellHistory.attrs}
              checked={terminals.inheritGlobalShellHistory}
            />{' '}
            Use my global shell history{' '}
            <span>
              By default, each terminal keeps private, machine-local command recall for this project and terminal. This
              opt-out applies only on this machine and affects newly created terminals.
            </span>
          </label>
          <p role="status">{terminals.message}</p>
        </>
      )}
      {category === 'permissions' && (
        <div class="project-settings__permission-grid">
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
          <p>
            Runs only while this project's floating permission popup is visible. Ignore pauses the timer; Stop
            auto-allow or Stop auto-deny disables that automatic decision for this request. Auto-allow after 0 seconds
            allows each request without showing the popup; it is still recorded in Notifications.
          </p>
        </div>
      )}
      {category === 'columns' && (
        <label class="project-settings__option">
          <input
            type="checkbox"
            {...NOTIFICATIONS_AND_LINKS_ACTIONS.toggleVerifiedColumn.attrs}
            checked={columns.hideVerified}
          />{' '}
          Hide Verified column <span>Verified tickets appear in Completed.</span>
        </label>
      )}
      {category === 'general' && (
        <label class="project-settings__option">
          <input
            type="checkbox"
            {...COMMANDS_AND_AI_ACTIONS.toggleLoadingActivity.attrs}
            checked={general.showLoadingActivity}
          />{' '}
          Show loading activity{' '}
          <span>
            Shows a small label at the top of the app describing what the server is doing (for example “Loading
            tickets”). This applies only on this machine.
          </span>
        </label>
      )}
      {category === 'keyboard' && <KeyboardSettings {...keyboard} />}
    </section>
  );
}
