import { Select } from '@kerfjs/ui/select';
import { signal } from 'kerfjs';

import { type SettingsCategory, settingsCategoryTitle } from '../components/settings-navigation';
import type { PermissionAutomationAction } from '../permission-notifications';
import { DEMO_ACTIONS } from './demo-actions';
import { syncSettingsControls } from './settings-controls';

/** Every SettingsWorkspace category, in navigator order: project categories, then app categories (HS2-PS9BQV). */
export const SETTINGS_WORKSPACE_DEMO_CATEGORIES: readonly SettingsCategory[] = [
  'sources',
  'ai',
  'commands',
  'lifecycle',
  'terminals',
  'permissions',
  'columns',
  'general',
  'accounts',
  'keyboard',
];

export const settingsWorkspaceSettings = {
  category: signal<SettingsCategory>('sources'),
  permissionAction: signal<PermissionAutomationAction>('off'),
};

export function resetSettingsWorkspaceDemo(root: ParentNode): void {
  settingsWorkspaceSettings.category.value = 'sources';
  settingsWorkspaceSettings.permissionAction.value = 'off';
  syncSettingsControls(root, 'settings-workspace', {
    values: { 'workspace-category': 'sources', 'workspace-permission-action': 'off' },
  });
}

export function SettingsWorkspaceSettings() {
  return (
    <form class="settings-form" data-settings="settings-workspace">
      <Select
        name="workspace-category"
        label="Category"
        value={settingsWorkspaceSettings.category.value}
        choices={SETTINGS_WORKSPACE_DEMO_CATEGORIES.map((value) => ({ value, label: settingsCategoryTitle(value) }))}
      />
      <Select
        name="workspace-permission-action"
        label="Permission automation"
        hint="Applies to the Permissions category: any decision other than Off enables the delay."
        value={settingsWorkspaceSettings.permissionAction.value}
        choices={[
          { value: 'off', label: 'Off' },
          { value: 'allow', label: 'Auto-allow' },
          { value: 'deny', label: 'Auto-deny' },
        ]}
      />
      <wa-button type="button" {...DEMO_ACTIONS.resetSettings.attrs}>
        Reset
      </wa-button>
    </form>
  );
}
