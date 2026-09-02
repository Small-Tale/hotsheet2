import './settings-navigation.css';

import { Columns3, Database, PanelLeftClose, ShieldCheck, TerminalSquare } from 'lucide';

import { LucideIcon } from './lucide-icon';
import { MenuItem } from './menu-item';
import { Toolbar } from './toolbar';
import { ToolbarControlGroup } from './toolbar-control-group';

export type SettingsCategory = 'sources' | 'commands' | 'permissions' | 'columns';

const categories = [
  { id: 'sources', label: 'Ticket sources', icon: Database, iconName: 'database' },
  { id: 'commands', label: 'Commands', icon: TerminalSquare, iconName: 'terminal-square' },
  { id: 'permissions', label: 'Permissions', icon: ShieldCheck, iconName: 'shield-check' },
  { id: 'columns', label: 'Column view', icon: Columns3, iconName: 'columns-3' },
] as const;

export function settingsCategoryTitle(category: SettingsCategory): string {
  return categories.find(item => item.id === category)?.label ?? 'Settings';
}

export function SettingsNavigation({ selected, collapseControl = false }: { selected: SettingsCategory; collapseControl?: boolean }) {
  return <aside class="settings-navigation" data-component="settings-navigation" aria-label="Settings categories">
    {collapseControl && <Toolbar divider={false} trailing={<ToolbarControlGroup appearance="borderless" single><button type="button" data-action="toggle-project-sidebar" aria-label="Hide settings sidebar" title="Hide settings sidebar"><LucideIcon icon={PanelLeftClose} name="panel-left-close" /></button></ToolbarControlGroup>} />}
    <div class="settings-navigation__content">
      <p class="settings-navigation__heading">Settings</p>
      <nav aria-label="Settings categories">
        {categories.map(item => <MenuItem action="select-settings-category" itemId={item.id} selected={selected === item.id} icon={<LucideIcon icon={item.icon} name={item.iconName} />} label={item.label} />)}
      </nav>
    </div>
  </aside>;
}
