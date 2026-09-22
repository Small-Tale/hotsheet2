import { wireTabBars } from '@kerfjs/ui/wire-tab-bars';
import { delegate, type Signal } from 'kerfjs';

import { type CorruptTicket, type FullTicket } from '../api';
import { corruptTicketKey } from '../components/corrupt-ticket-row';
import { PROJECT_TAB_BAR_ID, type ProjectTabBarMode } from '../components/project-tab-bar';
import { TERMINAL_DRAWER_TAB_BAR_ID } from '../components/terminal-drawer';
import { reorderDrawerTabIds } from '../drawer-tab-order';
import { reorderTabs } from '../tab-order';
import { type TicketView } from '../ticket-views';
import { data } from './dom';
import { type Control, type Project } from './types';

/** Live application bindings used by this handler group. */
export interface NavigationAndTabInteractionsDependencies {
  readonly projects: Signal<Project[]>;
  readonly currentRememberedProjectRoots: () => string[];
  readonly project: () => Project | undefined;
  readonly persistDrawerTabOrder: (projectId: string, ids: readonly string[]) => void;
  readonly currentDrawerTabIds: (projectId: string) => string[];
  readonly focusDrawerTab: (projectId: string, id: string) => void;
  readonly revealCorruptTicket: (key: string) => Promise<void>;
  readonly queueCorruptTicketRepair: (key: string) => Promise<void>;
  readonly corruptTickets: Signal<CorruptTicket[]>;
  readonly selectedCorruptKey: Signal<string | undefined>;
  readonly selectedTicket: Signal<FullTicket | null>;
  readonly selectedTicketSlugs: Signal<string[]>;
  ticketSelectionAnchor: string | undefined;
  readonly setInspectorVisible: (visible: boolean) => void;
  readonly error: Signal<string>;
  readonly statsProjectId: Signal<string | undefined>;
  readonly setShellMode: (mode: ProjectTabBarMode) => void;
  readonly selectTerminalRailProject: (next: string) => void;
  readonly selectTicketView: (next: TicketView, { refresh }?: { refresh?: boolean }) => void;
  readonly terminalRailDirection: Signal<'forward' | 'backward'>;
  readonly terminalRailScreen: Signal<'root' | 'ticket'>;
  readonly selectProjectTab: (next: string) => void;
  readonly retryProjectRestore: (root: string) => Promise<void>;
}

/** Register this group only when the application wiring owner invokes it. */
export function wireNavigationAndTabInteractions(dependencies: NavigationAndTabInteractionsDependencies) {
  const {
    projects,
    currentRememberedProjectRoots,
    project,
    persistDrawerTabOrder,
    currentDrawerTabIds,
    focusDrawerTab,
    revealCorruptTicket,
    queueCorruptTicketRepair,
    corruptTickets,
    selectedCorruptKey,
    selectedTicket,
    selectedTicketSlugs,
    setInspectorVisible,
    error,
    statsProjectId,
    setShellMode,
    selectTerminalRailProject,
    selectTicketView,
    terminalRailDirection,
    terminalRailScreen,
    selectProjectTab,
    retryProjectRestore,
  } = dependencies;
  wireTabBars(document.body, {
    activation: 'manual',
    onReorder: ({ barId, sourceId, targetId, position }) => {
      if (barId === PROJECT_TAB_BAR_ID) {
        projects.value = reorderTabs(projects.value, (item) => item.id, sourceId, targetId, position);
        localStorage.setItem('hotsheet.open-projects', JSON.stringify(currentRememberedProjectRoots()));
        return;
      }
      if (barId !== TERMINAL_DRAWER_TAB_BAR_ID) return;
      const current = project();
      if (!current) return;
      persistDrawerTabOrder(
        current.id,
        reorderDrawerTabIds(currentDrawerTabIds(current.id), sourceId, targetId, position),
      );
      focusDrawerTab(current.id, sourceId);
    },
  });
  delegate(document.body, 'keydown', '[role="tab"]', (event, target) => {
    const keyboard = event as KeyboardEvent,
      key = keyboard.key;
    if (event.defaultPrevented) return;
    if ((key === 'Delete' || key === 'Backspace') && target.closest('[data-tab-kind]')) {
      event.preventDefault();
      target.closest<HTMLElement>('[data-tab-kind]')?.querySelector<HTMLButtonElement>('.kui-app-tab__close')?.click();
      return;
    }
    if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(key)) return;
    if (target.closest('[data-component="tab-bar"]')) return;
    const tablist = target.closest<HTMLElement>('[role="tablist"]');
    if (!tablist) return;
    const tabs = [...tablist.querySelectorAll<HTMLElement>('[role="tab"]')].filter(
        (tab) => tab.closest('[role="tablist"]') === tablist && !tab.hasAttribute('disabled'),
      ),
      index = tabs.indexOf(target as HTMLElement);
    if (index < 0 || tabs.length < 2) return;
    event.preventDefault();
    const next =
      key === 'Home'
        ? 0
        : key === 'End'
          ? tabs.length - 1
          : key === 'ArrowLeft'
            ? (index - 1 + tabs.length) % tabs.length
            : (index + 1) % tabs.length;
    tabs[next].focus();
  });
  delegate(document.body, 'click', '[data-action="reveal-corrupt-ticket"]', (_event, target) => {
    void revealCorruptTicket(data(target).corruptKey!);
  });
  delegate(document.body, 'click', '[data-action="repair-corrupt-ticket"]', (_event, target) => {
    void queueCorruptTicketRepair(data(target).corruptKey!);
  });
  delegate(document.body, 'click', '[data-action="select-corrupt-ticket"]', (_event, target) => {
    const key = data(target).corruptKey;
    if (!key || !corruptTickets.value.some((ticket) => corruptTicketKey(ticket) === key)) return;
    selectedCorruptKey.value = key;
    selectedTicket.value = null;
    selectedTicketSlugs.value = [];
    dependencies.ticketSelectionAnchor = undefined;
    setInspectorVisible(true);
    error.value = '';
  });
  delegate(document.body, 'click', '[data-action="set-shell-mode"]', (_event, target) => {
    statsProjectId.value = undefined;
    setShellMode(data(target).shellMode as ProjectTabBarMode);
  });
  delegate(document.body, 'change', 'wa-select[name="terminal-rail-project"]', (_event, target) => {
    selectTerminalRailProject((target as Control).value);
  });
  delegate(document.body, 'change', 'wa-select[name="terminal-rail-view"]', (_event, target) => {
    selectTicketView((target as Control).value as TicketView);
  });
  delegate(document.body, 'change', 'wa-select[name="mobile-view"]', (_event, target) => {
    selectTicketView((target as Control).value as TicketView);
  });
  delegate(document.body, 'click', '[data-action="back-terminal-ticket-rail"]', () => {
    terminalRailDirection.value = 'backward';
    terminalRailScreen.value = 'root';
  });
  delegate(document.body, 'click', '[data-action="open-project-stats"]', (_event, target) => {
    const current = project();
    if (!current) return;
    const requested = data(target).projectId;
    statsProjectId.value = requested === 'all' ? undefined : requested || current.id;
    setShellMode('stats');
  });
  delegate(document.body, 'click', '[data-action="select-project-tab"]', (_event, target) => {
    selectProjectTab(data(target.closest<HTMLElement>('[data-tab-kind="project"]')!).projectId!);
  });
  delegate(document.body, 'change', 'wa-select[name="mobile-project"]', (_event, target) => {
    selectProjectTab((target as Control).value);
  });
  delegate(document.body, 'click', '[data-action="retry-project-restore"]', (_event, target) => {
    const root = data(target).projectRoot;
    if (root) void retryProjectRestore(root);
  });
}
