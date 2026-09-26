import { delegate, delegateCapture, type Signal } from 'kerfjs';

import { type AiToolDefaults } from '../api';
import { browserRandomId } from '../browser-id';
import { type ProjectCloseDialogState } from '../components/project-close-dialog';
import { type AppTabKind } from '../components/project-tab-context-menu';
import { type TerminalDashboardGroup, type TerminalDashboardSession } from '../components/terminal-dashboard';
import { type TerminalVisibilityNamePrompt } from '../components/terminal-visibility-dialog';
import { viewportSafeContextMenuPosition } from '../context-menu-position';
import { type DrawerTabCloseAction, drawerTabCloseIds } from '../drawer-tab-order';
import { type DrawerAIChat } from '../project-drive';
import { adjustTerminalFit, terminalGridBasis } from '../terminal-grid-layout';
import { type TerminalFocusRequest } from '../terminal-viewport';
import {
  activeTerminalVisibilityGroup,
  addTerminalVisibilityGroup,
  removeTerminalVisibilityGroup,
  renameTerminalVisibilityGroup,
  selectTerminalVisibilityGroup,
  setAllTerminalsVisibleInGroup,
  setTerminalVisibleInGroup,
  TERMINAL_VISIBILITY_TYPES,
  type TerminalVisibilityState,
  type TerminalVisibilityType,
} from '../terminal-visibility';
import { wireTerminalVisibilityTypeFilter } from '../terminal-visibility-filter';
import { data } from './dom';
import { type Control, type Project } from './types';

/** Live application bindings used by this handler group. */
export interface TerminalInteractionsDependencies {
  readonly terminalDrawerBounds: Signal<{ width: number; height: number }>;
  readonly terminalDashboardSize: Signal<{ width: number; height: number }>;
  readonly terminalDrawerFitHigh: Signal<number>;
  readonly terminalFitAcross: Signal<number>;
  readonly terminalFitHigh: Signal<number>;
  terminalPreviewClickTimer: number | undefined;
  readonly terminalSession: (key?: string) => TerminalDashboardSession | undefined;
  pendingTerminalFocus: TerminalFocusRequest | undefined;
  readonly magnifiedTerminalKey: Signal<string | undefined>;
  readonly openTerminalInProject: (key: string) => void;
  readonly terminalContextMenu: Signal<{ key: string; x: number; y: number } | undefined>;
  readonly terminalVisibilityScopeFor: (target: Element) => string;
  readonly terminalVisibility: Signal<TerminalVisibilityState>;
  readonly persistTerminalVisibility: (next: TerminalVisibilityState) => void;
  readonly terminalVisibilityFilter: Signal<readonly TerminalVisibilityType[]>;
  readonly terminalVisibilityContextMenu: Signal<{ id: string; x: number; y: number } | undefined>;
  readonly terminalVisibilityDialogScope: Signal<string | undefined>;
  readonly terminalVisibilityNamePrompt: Signal<TerminalVisibilityNamePrompt | undefined>;
  readonly terminalKeysForVisibilityDialog: () => string[];
  readonly openGridAIChat: (projectId: string, chatId: string) => void;
  readonly setTerminalDrawerVisible: (visible: boolean, refresh?: boolean) => void;
  readonly terminalDrawerVisible: Signal<boolean>;
  readonly toggleTerminalDrawerMaximized: () => void;
  readonly selectDrawerItem: (id: string) => void;
  readonly terminalDrawerCreateMenuOpen: Signal<boolean>;
  readonly enterMobileTerminalFocus: (terminalId: string) => void;
  readonly exitMobileTerminalFocus: () => void;
  readonly cycleMobileTerminalColumns: () => void;
  readonly focusDrawerTab: (projectId: string, id: string) => void;
  readonly createProjectTerminal: (selection?: AiToolDefaults) => Promise<void>;
  readonly aiLaunchConfiguration: (kind: 'ai-shell' | 'ai-chat', customize: boolean) => AiToolDefaults | undefined;
  readonly createDrawerAIChat: (
    selection: AiToolDefaults,
    options?: { connectionId?: string; drive?: boolean },
  ) => Promise<
    | DrawerAIChat
    | {
        id: string;
        connectionId: string;
        tool: string;
        name: string;
        model: string | undefined;
        effort: string | undefined;
        drive: boolean | undefined;
      }
    | undefined
  >;
  readonly openSavedConversation: () => Promise<void>;
  readonly requestProjectClose: (ids: readonly string[]) => void;
  readonly projectCloseDialog: Signal<ProjectCloseDialogState | undefined>;
  readonly restoreBorrowedProjectCloseTerminal: (state: ProjectCloseDialogState | undefined) => void;
  readonly cancelProjectClose: () => void;
  readonly confirmProjectClose: () => void;
  readonly closeAllProjectResources: () => Promise<void>;
  readonly closeTerminalIds: (ids: readonly string[]) => Promise<void>;
  readonly closeDrawerAIChat: (id: string) => void;
  readonly appTabContextMenu: Signal<
    { x: number; y: number; kind: AppTabKind; id: string; direction: 'left' | 'right' } | undefined
  >;
  readonly projects: Signal<Project[]>;
  readonly currentDrawerTabIds: (projectId: string) => string[];
  readonly project: () => Project | undefined;
  readonly terminalGroups: Signal<TerminalDashboardGroup[]>;
  readonly terminalRename: Signal<{ projectId: string; terminalId: string; value: string } | undefined>;
  readonly closeDrawerTabIds: (ids: readonly string[]) => Promise<void>;
  readonly saveTerminalName: (projectId: string, terminalId: string, name: string) => void;
}

/** Register this group only when the application wiring owner invokes it. */
export function wireTerminalInteractions(dependencies: TerminalInteractionsDependencies) {
  const {
    terminalDrawerBounds,
    terminalDashboardSize,
    terminalDrawerFitHigh,
    terminalFitAcross,
    terminalFitHigh,
    terminalSession,
    magnifiedTerminalKey,
    openTerminalInProject,
    terminalContextMenu,
    terminalVisibilityScopeFor,
    terminalVisibility,
    persistTerminalVisibility,
    terminalVisibilityFilter,
    terminalVisibilityContextMenu,
    terminalVisibilityDialogScope,
    terminalVisibilityNamePrompt,
    terminalKeysForVisibilityDialog,
    openGridAIChat,
    setTerminalDrawerVisible,
    terminalDrawerVisible,
    toggleTerminalDrawerMaximized,
    selectDrawerItem,
    terminalDrawerCreateMenuOpen,
    enterMobileTerminalFocus,
    exitMobileTerminalFocus,
    cycleMobileTerminalColumns,
    focusDrawerTab,
    createProjectTerminal,
    aiLaunchConfiguration,
    createDrawerAIChat,
    openSavedConversation,
    requestProjectClose,
    projectCloseDialog,
    restoreBorrowedProjectCloseTerminal,
    cancelProjectClose,
    confirmProjectClose,
    closeAllProjectResources,
    closeTerminalIds,
    closeDrawerAIChat,
    appTabContextMenu,
    projects,
    currentDrawerTabIds,
    project,
    terminalGroups,
    terminalRename,
    closeDrawerTabIds,
    saveTerminalName,
  } = dependencies;
  delegate(document.body, 'focusin', '.terminal-session:not([hidden]) .xterm-helper-textarea', (_event, target) => {
    const viewport = target.closest<HTMLElement>('[data-terminal-id]');
    if (!viewport?.closest('[data-component="terminal-drawer"][data-mode="dedicated"]')) return;
    enterMobileTerminalFocus(viewport.dataset.terminalId!);
  });
  delegate(document.body, 'click', '[data-action="exit-terminal-focus-mode"]', () => {
    const current = project(),
      drawer = document.querySelector<HTMLElement>('[data-component="terminal-drawer"]'),
      terminalId = drawer?.querySelector<HTMLElement>('.terminal-session:not([hidden]) [data-terminal-id]')?.dataset
        .terminalId;
    exitMobileTerminalFocus();
    if (current && drawer?.dataset.mode === 'dedicated' && terminalId) focusDrawerTab(current.id, terminalId);
  });
  delegate(document.body, 'click', '[data-action="zoom-terminal-grid"]', (_event, target) => {
    const drawer = Boolean(target.closest('[data-component="terminal-drawer"]')),
      bounds = drawer ? terminalDrawerBounds.value : terminalDashboardSize.value,
      basis = drawer ? 'high' : terminalGridBasis(bounds.height),
      direction = data(target).zoomDirection as 'in' | 'out';
    if (drawer) {
      terminalDrawerFitHigh.value = adjustTerminalFit(terminalDrawerFitHigh.value, basis, direction);
      localStorage.setItem('hotsheet.terminals.drawer-fit-high', String(terminalDrawerFitHigh.value));
    } else if (basis === 'across') {
      terminalFitAcross.value = adjustTerminalFit(terminalFitAcross.value, basis, direction);
      localStorage.setItem('hotsheet.terminals.fit-across', String(terminalFitAcross.value));
    } else {
      terminalFitHigh.value = adjustTerminalFit(terminalFitHigh.value, basis, direction);
      localStorage.setItem('hotsheet.terminals.fit-high', String(terminalFitHigh.value));
    }
  });
  delegate(document.body, 'click', '[data-action="preview-terminal"]', (event, target) => {
    if ((event.target as Element).closest('button') || (event as MouseEvent).detail > 1) return;
    if (dependencies.terminalPreviewClickTimer !== undefined)
      window.clearTimeout(dependencies.terminalPreviewClickTimer);
    const key = data(target).terminalKey;
    dependencies.terminalPreviewClickTimer = window.setTimeout(() => {
      dependencies.terminalPreviewClickTimer = undefined;
      const session = terminalSession(key);
      if (!session) return;
      dependencies.pendingTerminalFocus = { projectId: session.projectId, terminalId: session.id };
      magnifiedTerminalKey.value = key;
    }, 220);
  });
  delegate(document.body, 'keydown', '[data-action="preview-terminal"]', (event, target) => {
    const keyboard = event as KeyboardEvent;
    if (keyboard.key !== 'Enter' && keyboard.key !== ' ') return;
    event.preventDefault();
    const key = data(target).terminalKey,
      session = terminalSession(key);
    if (!session) return;
    dependencies.pendingTerminalFocus = { projectId: session.projectId, terminalId: session.id };
    magnifiedTerminalKey.value = key;
  });
  delegate(document.body, 'dblclick', '[data-component="terminal-tile"]', (event, target) => {
    if (data(target).magnified === 'true') return;
    event.preventDefault();
    if (dependencies.terminalPreviewClickTimer !== undefined) {
      window.clearTimeout(dependencies.terminalPreviewClickTimer);
      dependencies.terminalPreviewClickTimer = undefined;
    }
    openTerminalInProject(data(target).terminalKey!);
  });
  delegate(document.body, 'dblclick', '.terminal-dashboard__magnified .terminal-tile__footer', (event, target) => {
    if ((event.target as Element).closest('button')) return;
    event.preventDefault();
    openTerminalInProject(data(target.closest('[data-component="terminal-tile"]')!).terminalKey!);
  });
  delegate(document.body, 'contextmenu', '[data-component="terminal-tile"]', (event, target) => {
    if (data(target).magnified === 'true') return;
    event.preventDefault();
    const pointer = event as MouseEvent;
    terminalContextMenu.value = {
      key: data(target).terminalKey!,
      ...viewportSafeContextMenuPosition(pointer.clientX, pointer.clientY, window.innerWidth, window.innerHeight, {
        width: 224,
        height: 104,
      }),
    };
  });
  delegate(document.body, 'click', '[data-action="open-terminal-context-menu"]', (event, target) => {
    event.preventDefault();
    const box = target.getBoundingClientRect();
    terminalContextMenu.value = {
      key: data(target).itemId!,
      ...viewportSafeContextMenuPosition(box.right, box.bottom, window.innerWidth, window.innerHeight, {
        width: 224,
        height: 104,
      }),
    };
  });
  delegate(document.body, 'click', '[data-action="dismiss-magnified-terminal"]', (event, target) => {
    if (event.target === target) magnifiedTerminalKey.value = undefined;
  });
  delegate(document.body, 'click', '[data-action="close-magnified-terminal"]', () => {
    magnifiedTerminalKey.value = undefined;
  });
  delegate(document.body, 'click', '[data-action="cycle-mobile-terminal-columns"]', () => {
    cycleMobileTerminalColumns();
  });
  delegate(document.body, 'click', '[data-action="hide-dashboard-terminal"]', (_event, target) => {
    const key = data(target).terminalKey ?? data(target).itemId,
      scope = terminalVisibilityScopeFor(target),
      active = activeTerminalVisibilityGroup(terminalVisibility.value, scope);
    if (key) persistTerminalVisibility(setTerminalVisibleInGroup(terminalVisibility.value, active.id, key, false));
    terminalContextMenu.value = undefined;
    if (magnifiedTerminalKey.value === key) magnifiedTerminalKey.value = undefined;
  });
  wireTerminalVisibilityTypeFilter(document.body, (types) => {
    terminalVisibilityFilter.value = types;
  });
  delegate(document.body, 'click', '[data-action="open-terminal-visibility"]', (event, target) => {
    event.stopImmediatePropagation();
    terminalVisibilityContextMenu.value = undefined;
    terminalVisibilityFilter.value = TERMINAL_VISIBILITY_TYPES;
    terminalVisibilityDialogScope.value = terminalVisibilityScopeFor(target);
  });
  delegate(document.body, 'wa-hide', '[data-terminal-visibility-dialog]', (event, target) => {
    if (event.target !== target) return;
    terminalVisibilityContextMenu.value = undefined;
    terminalVisibilityNamePrompt.value = undefined;
    terminalVisibilityDialogScope.value = undefined;
  });
  delegate(document.body, 'change', '[name="terminal-visibility-group"]', (_event, target) => {
    const scope = terminalVisibilityScopeFor(target),
      id = (target as Control).value;
    persistTerminalVisibility(selectTerminalVisibilityGroup(terminalVisibility.value, scope, id));
  });
  delegate(document.body, 'click', '[data-action="select-terminal-visibility-tab"]', (_event, target) => {
    const scope = terminalVisibilityDialogScope.value,
      id = data(target).itemId;
    terminalVisibilityContextMenu.value = undefined;
    if (scope && id) persistTerminalVisibility(selectTerminalVisibilityGroup(terminalVisibility.value, scope, id));
  });
  delegate(document.body, 'contextmenu', '[data-visibility-group-id]', (event, target) => {
    const id = data(target).visibilityGroupId;
    if (!id || id === 'default') return;
    event.preventDefault();
    const pointer = event as MouseEvent;
    terminalVisibilityContextMenu.value = {
      id,
      ...viewportSafeContextMenuPosition(pointer.clientX, pointer.clientY, window.innerWidth, window.innerHeight, {
        width: 192,
        height: 96,
      }),
    };
  });
  delegate(document.body, 'click', '[data-action="add-terminal-visibility-group"]', () => {
    terminalVisibilityContextMenu.value = undefined;
    terminalVisibilityNamePrompt.value = { mode: 'add', value: '' };
    requestAnimationFrame(() =>
      document
        .querySelector<HTMLElement>('[data-terminal-visibility-name-dialog] [name="terminal-visibility-group-name"]')
        ?.focus(),
    );
  });
  delegate(document.body, 'click', '[data-action="rename-terminal-visibility-group"]', () => {
    const menu = terminalVisibilityContextMenu.value,
      group = terminalVisibility.value.groups.find((item) => item.id === menu?.id);
    terminalVisibilityContextMenu.value = undefined;
    if (!group) return;
    terminalVisibilityNamePrompt.value = { mode: 'rename', groupId: group.id, value: group.name };
    requestAnimationFrame(() =>
      document
        .querySelector<HTMLElement>('[data-terminal-visibility-name-dialog] [name="terminal-visibility-group-name"]')
        ?.focus(),
    );
  });
  delegate(document.body, 'click', '[data-action="remove-terminal-visibility-group"]', () => {
    const id = terminalVisibilityContextMenu.value?.id;
    terminalVisibilityContextMenu.value = undefined;
    if (id) persistTerminalVisibility(removeTerminalVisibilityGroup(terminalVisibility.value, id));
  });
  delegate(document.body, 'submit', '[data-action="submit-terminal-visibility-name"]', (event, target) => {
    event.preventDefault();
    const prompt = terminalVisibilityNamePrompt.value,
      scope = terminalVisibilityDialogScope.value,
      name = target.querySelector<Control>('[name="terminal-visibility-group-name"]')?.value.trim();
    if (!prompt || !scope || !name) return;
    if (prompt.mode === 'add') {
      const added = addTerminalVisibilityGroup(terminalVisibility.value, browserRandomId(), name);
      persistTerminalVisibility(selectTerminalVisibilityGroup(added.state, scope, added.group.id));
    } else if (prompt.groupId)
      persistTerminalVisibility(renameTerminalVisibilityGroup(terminalVisibility.value, prompt.groupId, name));
    terminalVisibilityNamePrompt.value = undefined;
  });
  delegate(document.body, 'click', '[data-action="cancel-terminal-visibility-name"]', () => {
    terminalVisibilityNamePrompt.value = undefined;
  });
  delegate(document.body, 'wa-hide', '[data-terminal-visibility-name-dialog]', () => {
    terminalVisibilityNamePrompt.value = undefined;
  });
  delegate(document.body, 'click', '[data-action="toggle-terminal-visibility"]', (_event, target) => {
    const scope = terminalVisibilityDialogScope.value,
      key = data(target).itemId;
    if (!scope || !key) return;
    const active = activeTerminalVisibilityGroup(terminalVisibility.value, scope),
      visible = active.hiddenKeys.includes(key);
    persistTerminalVisibility(setTerminalVisibleInGroup(terminalVisibility.value, active.id, key, visible));
  });
  delegate(
    document.body,
    'click',
    '[data-action="show-all-terminals-in-group"], [data-action="hide-all-terminals-in-group"]',
    (_event, target) => {
      const scope = terminalVisibilityDialogScope.value;
      if (!scope) return;
      const active = activeTerminalVisibilityGroup(terminalVisibility.value, scope),
        visible = data(target).action === 'show-all-terminals-in-group';
      persistTerminalVisibility(
        setAllTerminalsVisibleInGroup(terminalVisibility.value, active.id, terminalKeysForVisibilityDialog(), visible),
      );
    },
  );
  delegate(document.body, 'click', '[data-action="open-terminal-project"]', (_event, target) => {
    openTerminalInProject(data(target).terminalKey ?? data(target).itemId!);
  });
  delegate(document.body, 'click', '[data-action="open-grid-ai-chat"]', (_event, target) => {
    openGridAIChat(data(target).projectId!, data(target).chatId!);
  });
  delegate(document.body, 'keydown', '[data-component="workspace-chat-tile"]', (event, target) => {
    const keyboard = event as KeyboardEvent;
    if (keyboard.key !== 'Enter' && keyboard.key !== ' ') return;
    event.preventDefault();
    openGridAIChat(data(target).projectId!, data(target).chatId!);
  });
  delegate(document.body, 'click', '[data-action="toggle-terminal-drawer"]', () => {
    setTerminalDrawerVisible(!terminalDrawerVisible.value);
  });
  delegate(document.body, 'dblclick', '[data-action="toggle-terminal-drawer-maximize"]', (event) => {
    if ((event.target as Element).closest('button, input, textarea, select, a, [data-tab-kind="terminal"]')) return;
    toggleTerminalDrawerMaximized();
  });
  delegate(document.body, 'dblclick', '[data-tab-kind="terminal"], [data-action="select-drawer-item"]', (event) => {
    if ((event.target as Element).closest('[data-tab-kind="ai-chat"]')) return;
    event.stopPropagation();
    toggleTerminalDrawerMaximized();
  });
  delegate(document.body, 'click', '[data-action="select-drawer-item"]', (_event, target) => {
    const tab = target.closest<HTMLElement>('[data-tab-kind]');
    selectDrawerItem(tab?.dataset.terminalId || tab?.dataset.chatId || data(target).itemId || 'grid');
  });
  delegate(document.body, 'click', '[data-action="toggle-terminal-create-menu"]', (event) => {
    event.stopPropagation();
    terminalDrawerCreateMenuOpen.value = !terminalDrawerCreateMenuOpen.value;
  });
  delegate(document.body, 'click', '[data-action="create-terminal-drawer-item"]', (event, target) => {
    const kind = data(target).itemId as 'default-shell' | 'ai-shell' | 'ai-chat';
    terminalDrawerCreateMenuOpen.value = false;
    if (kind === 'default-shell') {
      void createProjectTerminal();
      return;
    }
    const configuration = aiLaunchConfiguration(kind, (event as MouseEvent).altKey);
    if (!configuration) return;
    if (kind === 'ai-shell') void createProjectTerminal(configuration);
    else void createDrawerAIChat(configuration);
  });
  delegate(document.body, 'click', '[data-action="open-saved-conversation"]', () => {
    void openSavedConversation();
  });
  document.addEventListener(
    'pointerdown',
    (event) => {
      if (terminalDrawerCreateMenuOpen.value && !(event.target as Element).closest('.terminal-drawer__create-wrap'))
        terminalDrawerCreateMenuOpen.value = false;
    },
    { capture: true },
  );
  delegate(document.body, 'click', '[data-action="create-project-terminal"]', () => {
    void createProjectTerminal();
  });
  delegate(document.body, 'click', '[data-action="close-project-tab"]', (event, target) => {
    event.stopPropagation();
    requestProjectClose([data(target.closest<HTMLElement>('[data-tab-kind="project"]')!).projectId!]);
  });
  delegate(document.body, 'click', '[data-action="select-project-close-resource"]', (_event, target) => {
    const state = projectCloseDialog.value,
      key = data(target).itemId;
    if (!state || !key || key === state.selectedKey) return;
    projectCloseDialog.value = { ...state, selectedKey: key, error: '' };
    restoreBorrowedProjectCloseTerminal(state);
  });
  delegate(document.body, 'click', '[data-action="cancel-project-close"]', () => {
    cancelProjectClose();
  });
  delegateCapture(document.body, 'wa-hide', '[data-component="project-close-dialog"]', () => {
    if (!projectCloseDialog.value?.operation) cancelProjectClose();
  });
  delegate(document.body, 'click', '[data-action="confirm-close-project"]', () => {
    confirmProjectClose();
  });
  delegate(document.body, 'click', '[data-action="close-all-project-resources"]', () => {
    void closeAllProjectResources();
  });
  delegate(document.body, 'click', '[data-action="close-terminal-tab"]', (event, target) => {
    event.stopPropagation();
    void closeTerminalIds([data(target.closest<HTMLElement>('[data-tab-kind="terminal"]')!).terminalId!]);
  });
  delegate(document.body, 'click', '[data-action="close-ai-chat-tab"]', (event, target) => {
    event.stopPropagation();
    closeDrawerAIChat(data(target.closest<HTMLElement>('[data-tab-kind="ai-chat"]')!).chatId!);
  });
  delegate(document.body, 'contextmenu', '[data-tab-kind]', (event, target) => {
    event.preventDefault();
    if (data(target).restoreFailure === 'true') return;
    const pointer = event as MouseEvent,
      kind = data(target).tabKind as AppTabKind,
      id =
        kind === 'project'
          ? data(target).projectId!
          : kind === 'ai-chat'
            ? data(target).chatId!
            : data(target).terminalId!;
    appTabContextMenu.value = {
      kind,
      id,
      direction: pointer.altKey ? 'left' : 'right',
      ...viewportSafeContextMenuPosition(pointer.clientX, pointer.clientY, window.innerWidth, window.innerHeight, {
        width: 256,
        height: 202,
      }),
    };
  });
  delegate(
    document.body,
    'click',
    '[data-action="project-tab-context-action"], [data-action="terminal-tab-context-action"]',
    (_event, target) => {
      const menu = appTabContextMenu.value;
      if (!menu) return;
      const ordered =
          menu.kind === 'project' ? projects.value.map((item) => item.id) : currentDrawerTabIds(project()?.id ?? ''),
        action = data(target).tabAction;
      if (action === 'rename' && menu.kind === 'terminal') {
        const group = terminalGroups.value.find((item) => item.projectId === project()?.id),
          session = group?.sessions.find((item) => item.id === menu.id);
        appTabContextMenu.value = undefined;
        if (session) {
          terminalRename.value = {
            projectId: session.projectId,
            terminalId: session.id,
            value: session.title ?? session.id,
          };
          queueMicrotask(() => document.querySelector<Control>('[name="terminal-name"]')?.focus());
        }
        return;
      }
      const ids = drawerTabCloseIds(ordered, menu.id, action as DrawerTabCloseAction);
      appTabContextMenu.value = undefined;
      if (menu.kind === 'project') requestProjectClose(ids);
      else void closeDrawerTabIds(ids);
    },
  );
  delegate(document.body, 'submit', '[data-action="rename-terminal-form"]', (event, target) => {
    event.preventDefault();
    const rename = terminalRename.value,
      name = target.querySelector<Control>('[name="terminal-name"]')?.value ?? '';
    if (!rename || !name.trim()) return;
    saveTerminalName(rename.projectId, rename.terminalId, name);
    terminalRename.value = undefined;
  });
  delegate(document.body, 'click', '[data-action="cancel-terminal-rename"]', () => {
    terminalRename.value = undefined;
  });
  delegate(document.body, 'wa-hide', '[data-terminal-rename-dialog]', () => {
    terminalRename.value = undefined;
  });
}
