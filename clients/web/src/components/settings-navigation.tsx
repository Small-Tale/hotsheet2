import '@kerfjs/ui/layout.css';
import './settings-navigation.css';

import { rem } from '@kerfjs/ui/css-values';
import { List } from '@kerfjs/ui/list';
import { ListHeader } from '@kerfjs/ui/list-header';
import { ListItem } from '@kerfjs/ui/list-item';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import {
  ArchiveRestore,
  Bot,
  Cable,
  Columns3,
  Database,
  Keyboard,
  ShieldCheck,
  SlidersHorizontal,
  TerminalSquare,
} from 'lucide';

import { SidebarPane, type SidebarPanelParts } from './sidebar-panel';

export type SettingsCategory =
  | 'sources'
  | 'ai'
  | 'commands'
  | 'lifecycle'
  | 'terminals'
  | 'permissions'
  | 'columns'
  | 'general'
  | 'connections'
  | 'keyboard';

/** Project-scoped settings, followed by app-scoped (device-local) settings. */
const projectCategories = [
  { id: 'sources', label: 'Ticket sources', icon: Database, iconName: 'database' },
  { id: 'ai', label: 'AI tools', icon: Bot, iconName: 'bot' },
  { id: 'commands', label: 'Commands', icon: TerminalSquare, iconName: 'terminal-square' },
  { id: 'lifecycle', label: 'Lifecycle', icon: ArchiveRestore, iconName: 'archive-restore' },
  { id: 'terminals', label: 'Terminals', icon: TerminalSquare, iconName: 'terminal-square' },
  { id: 'permissions', label: 'Permissions', icon: ShieldCheck, iconName: 'shield-check' },
  { id: 'columns', label: 'Column view', icon: Columns3, iconName: 'columns-3' },
] as const;

const appCategories = [
  { id: 'general', label: 'General', icon: SlidersHorizontal, iconName: 'sliders-horizontal' },
  // The machine-wide ticket-provider connection catalog, shared by every project (HS2-3SCH1K).
  { id: 'connections', label: 'Connections', icon: Cable, iconName: 'cable' },
  { id: 'keyboard', label: 'Keyboard', icon: Keyboard, iconName: 'keyboard' },
] as const;

const allCategories = [...projectCategories, ...appCategories];

/** Whether a settings category is app-scoped (device-local) rather than project-scoped. */
export function isAppSettingsCategory(category: SettingsCategory): boolean {
  return appCategories.some((item) => item.id === category);
}

export function settingsCategoryTitle(category: SettingsCategory): string {
  const item = allCategories.find((entry) => entry.id === category);
  if (!item) return 'Settings';
  return category === 'keyboard' ? 'Keyboard shortcuts' : item.label;
}

/** The settings navigator's panel parts for the Workbench's left rail (HS2-RWGQWN). */
export function settingsNavigationPanel({ selected }: { selected: SettingsCategory }): SidebarPanelParts {
  const renderGroup = (heading: string, items: readonly (typeof allCategories)[number][]) => (
    <section>
      <ListHeader label={heading} />
      <nav aria-label={heading}>
        <List gap={rem(0.125)}>
          {items.map((item) => (
            <ListItem
              action="select-settings-category"
              itemId={item.id}
              selected={selected === item.id}
              icon={<LucideIcon icon={item.icon} name={item.iconName} />}
              label={item.label}
            />
          ))}
        </List>
      </nav>
    </section>
  );
  return {
    label: 'Settings categories',
    toolbar: { label: 'Settings sidebar toolbar', dividerSides: '' },
    toggle: { action: 'toggle-project-sidebar', name: 'settings sidebar' },
    content: (
      <div class="settings-navigation__content">
        {renderGroup('Project Settings', projectCategories)}
        {renderGroup('App Settings', appCategories)}
      </div>
    ),
    pane: { contentElement: 'nav', contentLabel: 'Settings categories' },
  };
}

export function SettingsNavigation({
  selected,
  collapseControl = false,
}: {
  selected: SettingsCategory;
  collapseControl?: boolean;
}) {
  return (
    <SidebarPane
      parts={settingsNavigationPanel({ selected })}
      className="settings-navigation"
      collapseControl={collapseControl}
    />
  );
}
