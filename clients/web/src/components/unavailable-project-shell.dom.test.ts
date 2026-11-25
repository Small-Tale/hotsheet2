// HS2-H0QGK7: the shell keeps every region for a selected project that is not ready.
import { afterEach, beforeEach, expect, it } from 'vitest';

import { allFeaturesOn, featureSettings, resetFeatureSettings } from '../feature-areas';
import { unavailableProjectShell } from '../project-unavailable-shell';
import { AppShell } from './app-shell';
import { QuickTicketLauncher } from './quick-ticket-composer';
import {
  unavailableInspectorPanel,
  unavailableSidebarPanel,
  UnavailableTerminalDrawer,
} from './workspace-composition-surfaces';
import { WorkspaceControls } from './workspace-controls';

beforeEach(() => {
  featureSettings.value = allFeaturesOn();
});
afterEach(() => {
  resetFeatureSettings();
  document.body.replaceChildren();
});

it('HS2-H0QGK7 keeps every shell region for an unavailable project, empty, with controls disabled', () => {
  const unavailable = unavailableProjectShell('restore-failure', 'deleted-project'),
    reason = unavailable.reason;
  const render = (drawerVisible: boolean) => {
    document.body.innerHTML = String(
      AppShell({
        tabs: [],
        header: 'head' as never,
        workspace: 'work' as never,
        sidebar: unavailableSidebarPanel(unavailable.sidebar),
        inspector: unavailableInspectorPanel(unavailable.inspector),
        headerActions: WorkspaceControls({ mode: 'list', unavailableReason: reason }),
        projectTabAction: { kind: 'new-ticket', disabledReason: reason },
        terminalDrawer: UnavailableTerminalDrawer({ message: unavailable.drawer }),
        terminalDrawerVisible: drawerVisible,
        terminalDrawerDisabledReason: reason,
      }),
    );
    return document.body;
  };
  const root = render(false);
  // Every region keeps its place: both rails, the toolbar, and the drawer.
  for (const region of ['sidebar', 'inspector', 'drawer'])
    expect(root.querySelector(`[data-project-unavailable-region="${region}"]`)).not.toBeNull();
  expect(root.querySelector('[data-project-unavailable-region="sidebar"]')?.textContent).toContain(
    'Ticket views and commands appear here once deleted-project opens again.',
  );
  expect(
    root.querySelector('[data-project-unavailable-region="drawer"] [data-action="toggle-terminal-drawer"]'),
  ).not.toBeNull();
  // Rail toggles stay live; project controls are disabled with the reason.
  expect(root.querySelector<HTMLButtonElement>('[data-action="toggle-project-sidebar"]')?.disabled).toBe(false);
  const show = root.querySelector<HTMLButtonElement>('button[aria-label="Show Terminal Drawer"]')!;
  expect([show.disabled, show.title]).toEqual([true, reason]);
  const launcher = root.querySelector('[data-component="quick-ticket-composer-launcher"]')!;
  expect(launcher.hasAttribute('data-action')).toBe(false);
  expect(launcher.hasAttribute('data-ticket-drop-key')).toBe(false);
  expect(launcher.querySelector('wa-button, button')?.hasAttribute('disabled')).toBe(true);
  expect(
    root.querySelector('[data-component="segmented-control"], kui-segmented-control, [role="radiogroup"]'),
  ).not.toBeNull();
  expect(root.querySelector('[aria-label="View mode"]')?.getAttribute('aria-disabled')).toBe('true');
  expect(root.querySelector<HTMLButtonElement>('[data-action="toggle-selected-up-next"]')?.title).toBe(reason);
  expect(root.querySelector('[data-workspace-overflow] button[aria-haspopup="menu"]')?.hasAttribute('disabled')).toBe(
    true,
  );
  // An open drawer shows its empty state; the restore corner is not drawn while the drawer shows.
  expect(render(true).querySelector('[data-project-unavailable-region="drawer"]')).not.toBeNull();
  // A ready project's launcher keeps its action and drop target.
  document.body.innerHTML = String(QuickTicketLauncher({}));
  const ready = document.body.querySelector('[data-component="quick-ticket-composer-launcher"]')!;
  expect(ready.getAttribute('data-action')).toBe('expand-ticket-composer');
  expect(ready.hasAttribute('data-ticket-drop-key')).toBe(true);
});
