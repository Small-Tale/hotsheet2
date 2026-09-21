import './settings-workspace.css';

import type { AiToolDefaults, AiToolDescriptor } from '../api';
import type { PermissionAutomation } from '../permission-notifications';
import { AiToolSettings } from './ai-tool-settings';
import type { CommandSettingsEditorProps } from './command-settings-editor';
import { CommandSettingsEditor } from './command-settings-editor';
import { KeyboardSettings, type KeyboardSettingsProps } from './keyboard-settings';
import { type SettingsCategory, settingsCategoryTitle } from './settings-navigation';
import { TicketSourcesSettings, type TicketSourcesSettingsProps } from './ticket-sources-settings';
import { TrashSettings } from './trash-settings';

export interface SettingsWorkspaceProps {
  category: SettingsCategory;
  sources: TicketSourcesSettingsProps;
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
              data-action="toggle-global-shell-history"
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
          <wa-select
            name="permission-automation-action"
            label="Automatic decision"
            value={permissions.automation.action}
          >
            <wa-option value="off">Off</wa-option>
            <wa-option value="allow">Auto-allow</wa-option>
            <wa-option value="deny">Auto-deny</wa-option>
          </wa-select>
          <wa-select
            name="permission-automation-delay"
            label="After visible for"
            value={String(permissions.automation.delayMs)}
            disabled={permissions.automation.action === 'off'}
          >
            {permissions.delays.map((value) => (
              <wa-option value={String(value)}>
                {value < 60_000 ? '15 seconds' : `${value / 60_000} minute${value === 60_000 ? '' : 's'}`}
              </wa-option>
            ))}
          </wa-select>
          <p>
            Runs only while this project's floating permission popup is visible. Ignore pauses the timer; Stop
            auto-allow or Stop auto-deny disables that automatic decision for this request.
          </p>
        </div>
      )}
      {category === 'columns' && (
        <label class="project-settings__option">
          <input type="checkbox" data-action="toggle-verified-column" checked={columns.hideVerified} /> Hide Verified
          column <span>Verified tickets appear in Completed.</span>
        </label>
      )}
      {category === 'general' && (
        <label class="project-settings__option">
          <input type="checkbox" data-action="toggle-loading-activity" checked={general.showLoadingActivity} /> Show
          loading activity{' '}
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
