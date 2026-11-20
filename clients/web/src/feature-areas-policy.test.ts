import { readdirSync, readFileSync } from 'node:fs';
import { join, resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

import { SETTINGS_CATEGORY_FEATURES } from './components/settings-navigation';
import { INSPECTOR_TAB_FEATURES } from './components/ticket-inspector';
import { FEATURE_AREAS, isFeatureId, VIEW_MODE_FEATURES } from './feature-areas';
import { KEYBOARD_SHORTCUTS } from './keyboard-shortcuts';

/**
 * HS2-3GJZGG policy: every non-core entry point belongs to a registered feature area. A new strip
 * button, view-switcher choice, settings category, inspector tab, or keyboard shortcut fails here
 * until it is either added to the core list below (with a reason in review) or tagged with a
 * registered feature (`data-feature`, or the registry map its kind uses). The Features page and
 * docs/06-clients.md §6.12 list the areas.
 */
const SRC = resolve(__dirname);
const read = (path: string) => readFileSync(join(SRC, path), 'utf8');

/** Core entry points: always shown, never listed on the Features page. */
const CORE = {
  shortcuts: [
    'open-search',
    'undo',
    'redo',
    'toggle-left-sidebar',
    'toggle-right-sidebar',
    'toggle-bottom-drawer',
    'view-list',
    'view-board',
    'view-notifications',
    'view-settings',
    'select-all-tickets',
    'copy-tickets',
    'cut-tickets',
    'paste-tickets',
    'new-ticket',
    'move-selection-up',
    'move-selection-down',
    'activate',
    'dismiss',
    'tab-previous',
    'tab-next',
    'tab-first',
    'tab-last',
    'close-tab',
    'reorder-tab-left',
    'reorder-tab-right',
    'project-tab-previous',
    'project-tab-next',
    'drawer-tab-previous',
    'drawer-tab-next',
    'clear-terminal',
    'gallery-previous',
    'gallery-next',
    'gallery-toggle-playback',
  ],
  // Orchestration is shown only for an Orchestration project (HS2-J89JC7), not a feature area.
  viewModes: ['list', 'board', 'orchestration', 'notifications', 'settings'],
  settingsCategories: [
    'project-general',
    'orchestration',
    'sources',
    'ai',
    'guidance',
    'lifecycle',
    'terminals',
    'permissions',
    'development-strategy',
    'general',
    'accounts',
    'notifications',
    'terminal-memory',
    'devices',
    'projects',
    'keyboard',
    'features',
  ],
  inspectorTabs: ['info', 'attachments'],
} as const;

function sources(directory: string): string[] {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) return entry.name === 'ux-demo' ? [] : sources(path);
    return /\.tsx?$/.test(entry.name) && !/\.test\.tsx?$/.test(entry.name) ? [path] : [];
  });
}

describe('feature areas policy (HS2-3GJZGG)', () => {
  it('lists the same ids, groups and defaults as the server registry', () => {
    const rust = readFileSync(resolve(SRC, '../../../crates/hotsheet-ticketing/src/features.rs'), 'utf8');
    const autoGrouping = /pub const AUTO_GROUPING: &str = "([^"]+)";/.exec(rust)?.[1] ?? '';
    const named = rust.replaceAll('feature(AUTO_GROUPING,', `feature("${autoGrouping}",`);
    const server = [...named.matchAll(/feature\("([^"]+)", FeatureGroup::(\w+), (true|false)\)/g)].map((match) => ({
      id: match[1],
      group: match[2].toLowerCase(),
      defaultOn: match[3] === 'true',
    }));
    expect(server).toEqual(FEATURE_AREAS.map(({ id, group, defaultOn }) => ({ id, group, defaultOn })));
  });

  it('tags every keyboard shortcut that is not core with a registered feature', () => {
    for (const shortcut of KEYBOARD_SHORTCUTS) {
      const core = (CORE.shortcuts as readonly string[]).includes(shortcut.id);
      expect(core !== (shortcut.feature !== undefined), `shortcut ${shortcut.id}`).toBe(true);
    }
  });

  it('tags every view-switcher choice that is not core with a registered feature', () => {
    // The segmented switcher's modes and the compact overflow menu's mode list.
    const source = read('components/workspace-controls.tsx');
    const controls = [
      source.slice(
        source.indexOf('function workspaceModeChoices('),
        source.indexOf('return (', source.indexOf('function workspaceModeChoices(')),
      ),
      source.slice(source.indexOf("label: 'Show List View'") - 80, source.indexOf('const overflowIcon')),
    ].join('\n');
    const choices = new Set(
      [...controls.matchAll(/\{ value: '([\w-]+)'(?: as const)?, label:/g)].map((match) => match[1]),
    );
    expect(choices.size).toBeGreaterThan(4);
    for (const mode of choices) {
      const core = (CORE.viewModes as readonly string[]).includes(mode);
      expect(core !== mode in VIEW_MODE_FEATURES, `view ${mode}`).toBe(true);
    }
  });

  it('tags every settings category that is not core with a registered feature', () => {
    const navigation = read('components/settings-navigation.tsx');
    const union = /export type SettingsCategory =([^;]+);/.exec(navigation)?.[1] ?? '';
    const categories = [...union.matchAll(/'([\w-]+)'/g)].map((match) => match[1]);
    expect(categories.length).toBeGreaterThan(10);
    for (const category of categories) {
      const core = (CORE.settingsCategories as readonly string[]).includes(category);
      const feature = (SETTINGS_CATEGORY_FEATURES as Record<string, string | undefined>)[category];
      expect(core !== (feature !== undefined), `settings category ${category}`).toBe(true);
    }
  });

  it('tags every inspector tab that is not core with a registered feature', () => {
    const inspector = read('components/ticket-inspector.tsx');
    const union = /export type InspectorTab =([^;]+);/.exec(inspector)?.[1] ?? '';
    for (const tab of [...union.matchAll(/'([\w-]+)'/g)].map((match) => match[1])) {
      const core = (CORE.inspectorTabs as readonly string[]).includes(tab);
      const feature = (INSPECTOR_TAB_FEATURES as Record<string, string | undefined>)[tab];
      expect(core !== (feature !== undefined), `inspector tab ${tab}`).toBe(true);
    }
  });

  it('tags every project-strip dashboard button with its feature', () => {
    const strip = read('components/project-strip-actions.tsx');
    const buttons = [...strip.matchAll(/<button[^>]*?data-shell-mode="(\w+)"[^>]*?>/gs)];
    expect(buttons.length).toBeGreaterThan(0);
    for (const [button, mode] of buttons)
      expect(/data-feature="[\w-]+"/.test(button), `strip button ${mode}`).toBe(true);
  });

  it('names only registered features in data-feature attributes, and gates every registered feature', () => {
    const all = sources(SRC)
      .map((path) => readFileSync(path, 'utf8'))
      .join('\n');
    const named = [...all.matchAll(/data-feature(?:="|': ')([\w-]+)/g), ...all.matchAll(/featureOn\('([\w-]+)'/g)].map(
      (match) => match[1],
    );
    for (const id of named) expect(isFeatureId(id), `unregistered feature ${id}`).toBe(true);
    const mapped = [
      ...Object.values(VIEW_MODE_FEATURES),
      ...Object.values(SETTINGS_CATEGORY_FEATURES),
      ...Object.values(INSPECTOR_TAB_FEATURES),
      ...KEYBOARD_SHORTCUTS.flatMap((shortcut) => (shortcut.feature ? [shortcut.feature] : [])),
      // A dashboard shell mode, gated through shellModeFeature.
      'project-stats',
    ];
    for (const area of FEATURE_AREAS)
      expect(named.includes(area.id) || mapped.includes(area.id), `feature ${area.id} gates nothing`).toBe(true);
  });
});
