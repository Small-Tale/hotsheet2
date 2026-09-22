import '@kerfjs/ui/layout.css';
import './settings-navigation.css';

import { ListHeader } from '@kerfjs/ui/list-header';
import { ListItem } from '@kerfjs/ui/list-item';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Pane } from '@kerfjs/ui/pane';
import { Toolbar } from '@kerfjs/ui/toolbar';
import { ToolbarControlGroup } from '@kerfjs/ui/toolbar-control-group';
import {
  ArchiveRestore,
  Bot,
  Columns3,
  Database,
  Keyboard,
  PanelLeftClose,
  ShieldCheck,
  SlidersHorizontal,
  TerminalSquare,
} from 'lucide';

export type SettingsCategory =
  'sources' | 'ai' | 'commands' | 'lifecycle' | 'terminals' | 'permissions' | 'columns' | 'general' | 'keyboard';

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

export function SettingsNavigation({
  selected,
  collapseControl = false,
}: {
  selected: SettingsCategory;
  collapseControl?: boolean;
}) {
  const renderGroup = (heading: string, items: readonly (typeof allCategories)[number][]) => (
    <section>
      <ListHeader label={heading} />
      <nav aria-label={heading}>
        {items.map((item) => (
          <ListItem
            action="select-settings-category"
            itemId={item.id}
            selected={selected === item.id}
            icon={<LucideIcon icon={item.icon} name={item.iconName} />}
            label={item.label}
          />
        ))}
      </nav>
    </section>
  );
  const header = collapseControl ? (
    <Toolbar
      divider={false}
      trailing={
        <ToolbarControlGroup appearance="borderless" single>
          <button
            type="button"
            data-action="toggle-project-sidebar"
            aria-label="Hide settings sidebar"
            title="Hide settings sidebar"
          >
            <LucideIcon icon={PanelLeftClose} name="panel-left-close" />
          </button>
        </ToolbarControlGroup>
      }
    />
  ) : undefined;
  return (
    <Pane
      element="aside"
      label="Settings categories"
      className="settings-navigation"
      header={header}
      contentClassName="settings-navigation__content"
    >
      {renderGroup('Project Settings', projectCategories)}
      {renderGroup('App Settings', appCategories)}
    </Pane>
  );
}
