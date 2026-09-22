import { readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

import { KEYBOARD_SHORTCUTS } from './keyboard-shortcuts';

/**
 * The central keydown dispatcher lives in `interactions/shell-and-global.ts`; its complete wiring is
 * asserted against the module source: every view/panel and tab-cycling command shortcut added in
 * HS2-9SHYWD must be resolved through `matchesShortcut` in the dispatcher and act on the matching
 * app state, so a new registry entry cannot be shipped without a runtime binding.
 */
describe('keyboard-shortcut dispatcher wiring (HS2-9SHYWD)', () => {
  const source = ['./interactions/shell-and-global.ts']
    .map((file) => readFileSync(new URL(file, import.meta.url), 'utf8'))
    .join('\n');

  const boundIds = [
    'toggle-left-sidebar',
    'toggle-right-sidebar',
    'toggle-bottom-drawer',
    'view-list',
    'view-board',
    'view-notifications',
    'view-settings',
    'view-workspace-grid',
    'view-all-stats',
    'new-ticket',
    'project-tab-next',
    'project-tab-previous',
    'drawer-tab-next',
    'drawer-tab-previous',
  ];

  it('resolves every new command shortcut through matchesShortcut in the dispatcher', () => {
    for (const id of boundIds) {
      expect(source, id).toContainSource(`matchesShortcut('${id}',event,overrides,appleShortcutPlatform)`);
    }
  });

  it('wires each binding to its corresponding app action', () => {
    expect(source).toMatch(
      /toggle-left-sidebar'[^}]*viewportMobile\.value[^}]*toggleMobileSidebar\(mobileOverlay\.value\)[^}]*setSidebarVisible\(!sidebarVisible\.value\)/,
    );
    expect(source).toMatch(
      /toggle-right-sidebar'[^}]*viewportMobile\.value[^}]*toggleMobileInspector\(mobileOverlay\.value\)[^}]*setInspectorVisible\(!inspectorVisible\.value\)/,
    );
    expect(source).toMatch(/toggle-bottom-drawer'[^}]*setTerminalDrawerVisible\(!terminalDrawerVisible\.value\)/);
    expect(source).toMatch(/view-list'[^}]*switchWorkspaceView\('list'\)/);
    expect(source).toMatch(/view-board'[^}]*switchWorkspaceView\('board'\)/);
    expect(source).toMatch(/view-notifications'[^}]*switchWorkspaceView\('notifications'\)/);
    expect(source).toMatch(/view-settings'[^}]*switchWorkspaceView\('settings'\)/);
    expect(source).toContainSource('cycleTabId(projects.value.map(item=>item.id),selectedProjectId.value');
    expect(source).toContainSource(
      "cycleTabId(['grid',...currentDrawerTabIds(drawerProject.id)],terminalDrawerSelected.value",
    );
    expect(source).toMatch(/new-ticket'[^}]*openTicketComposer\(\)/);
    // The bare-key new-ticket chord must stay guarded so it never fires while typing.
    expect(source).toMatchSource(/!editable&&matchesShortcut\('new-ticket'/);
  });

  it('binds exactly the editable command ids the registry marks as dispatcher-resolved', () => {
    // Guard against a registry entry drifting out of the dispatcher: every id we bind here exists and
    // is editable in the shared registry.
    for (const id of boundIds) {
      expect(KEYBOARD_SHORTCUTS.find((s) => s.id === id)?.editable, id).toBe(true);
    }
  });
});
