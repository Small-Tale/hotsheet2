import { Button } from '@kerfjs/ui/button';
import { Select } from '@kerfjs/ui/select';
import { signal } from 'kerfjs';

import { type SettingsCategory, settingsCategoryTitle } from '../components/settings-navigation';
import type { PermissionAutomationAction } from '../permission-notifications';
import { DEMO_ACTIONS } from './demo-actions';
import { DemoSettingsForm } from './demo-settings-form';
import { syncSettingsControls } from './settings-controls';

/** Every SettingsWorkspace category, in navigator order: project categories, then app categories (HS2-PS9BQV). */
export const SETTINGS_WORKSPACE_DEMO_CATEGORIES: readonly SettingsCategory[] = [
  'sources',
  'ai',
  'guidance',
  'commands',
  'lifecycle',
  'terminals',
  'permissions',
  'columns',
  'general',
  'accounts',
  'terminal-memory',
  'keyboard',
];

export const settingsWorkspaceSettings = {
  category: signal<SettingsCategory>('sources'),
  permissionAction: signal<PermissionAutomationAction>('off'),
  guidanceSource: signal<'empty' | 'registered'>('empty'),
};

export function resetSettingsWorkspaceDemo(root: ParentNode): void {
  settingsWorkspaceSettings.category.value = 'sources';
  settingsWorkspaceSettings.permissionAction.value = 'off';
  settingsWorkspaceSettings.guidanceSource.value = 'empty';
  syncSettingsControls(root, 'settings-workspace', {
    values: {
      'workspace-category': 'sources',
      'workspace-permission-action': 'off',
      'workspace-guidance-source': 'empty',
    },
  });
}

export function SettingsWorkspaceSettings() {
  return (
    <DemoSettingsForm settings="settings-workspace">
      <Select
        name="workspace-category"
        label="Category"
        value={settingsWorkspaceSettings.category.value}
        choices={SETTINGS_WORKSPACE_DEMO_CATEGORIES.map((value) => ({ value, label: settingsCategoryTitle(value) }))}
      />
      <Select
        name="workspace-guidance-source"
        label="Guidance Source"
        hint="Applies to the Project guidance category."
        value={settingsWorkspaceSettings.guidanceSource.value}
        choices={[
          { value: 'empty', label: 'No Registered Source' },
          { value: 'registered', label: 'Registered Source' },
        ]}
      />
      <Select
        name="workspace-permission-action"
        label="Permission Automation"
        hint="Applies to the Permissions category: any decision other than Off enables the delay."
        value={settingsWorkspaceSettings.permissionAction.value}
        choices={[
          { value: 'off', label: 'Off' },
          { value: 'allow', label: 'Auto-Allow' },
          { value: 'deny', label: 'Auto-Deny' },
        ]}
      />
      <Button type="button" action={DEMO_ACTIONS.resetSettings.value}>
        Reset
      </Button>
    </DemoSettingsForm>
  );
}
