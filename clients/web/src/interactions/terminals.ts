import { placeSearchFieldCaret } from '@kerfjs/ui/search-field';
import { type SearchFieldModel } from '@kerfjs/ui/search-field-model';
import { delegate, delegateCapture, effect, type Signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import { type AiToolDefaults, Api } from '../api';
import { browserRandomId } from '../browser-id';
import { type ProjectCloseDialogState } from '../components/project-close-dialog';
import { type ProjectRestoreFailure, projectRestoreTabId } from '../components/project-restore-error';
import { type AppTabKind, type AppTabMenuState } from '../components/project-tab-context-menu';
import {
  type TerminalCopyState,
  type TerminalEditMenuState,
  type TerminalPasteState,
} from '../components/terminal-clipboard-dialogs';
import {
  wireTerminalClipboardActions,
  wireTerminalClipboardSheets,
  wireTerminalDashboardMenu,
  wireTerminalKeyBar,
  wireTerminalRenameDialog,
  wireTerminalTextSizeControl,
} from '../components/terminal-component-interactions';
import { type TerminalDashboardGroup, type TerminalDashboardSession } from '../components/terminal-dashboard';
import { type TerminalRenameTarget } from '../components/terminal-rename-dialog';
import { type TerminalVisibilityNamePrompt } from '../components/terminal-visibility-dialog';
import { revealContextPopupMenu, viewportSafeContextMenuPosition } from '../context-menu-position';
import { copyWithSelection } from '../copy-text';
import { type DrawerTabCloseAction, drawerTabCloseIds } from '../drawer-tab-order';
import { TERMINALS_ACTIONS, TERMINALS_TARGETS } from '../interaction-attrs/terminals';
import { isAppleShortcutPlatform } from '../keyboard-shortcuts';
import { type MobileTerminalFocusState } from '../mobile-terminal-focus';
import { closedPopupMenuRoot } from '../popup-menu-state';
import { type DrawerAIChat } from '../project-drive';
import { clearTerminalEverywhere, terminalClearKeyTarget, terminalViewportIdentity } from '../terminal-clear';
import {
  pasteIntoTerminalViewport,
  readClipboardText,
  readTerminalViewportSelection,
  readTerminalViewportText,
  TERMINAL_EDIT_MENU_EVENT,
  terminalCopyMessage,
  terminalCopySelection,
  type TerminalEditMenuDetail,
  terminalEditMenuDismissKey,
  terminalSelectionCopyKey,
  writeClipboardText,
} from '../terminal-clipboard';
import { type FeedbackPrimaryCommand, feedbackPrimaryMenuItem } from '../terminal-feedback-primary';
import {
  adjustTerminalFit,
  TERMINAL_GRID_MAX_PHONE,
  TERMINAL_GRID_PHONE_BREAKPOINT,
  terminalGridBasis,
} from '../terminal-grid-layout';
import {
  NO_TERMINAL_MODIFIERS,
  TERMINAL_KEY_EVENT,
  type TerminalModifiers,
  toggleTerminalModifier,
} from '../terminal-keys';
import {
  createTerminalSearchSync,
  TERMINAL_SEARCH_READY_EVENT,
  TERMINAL_SEARCH_RESULTS_EVENT,
  type TerminalSearchResults,
} from '../terminal-search';
import { type TerminalFocusRequest } from '../terminal-viewport';
import {
  activeTerminalVisibilityGroup,
  addTerminalVisibilityGroup,
  removeTerminalVisibilityGroup,
  renameTerminalVisibilityGroup,
  selectTerminalVisibilityGroup,
  setAllTerminalsVisibleInGroup,
  setTerminalVisibleInGroup,
  type TerminalVisibilityState,
  type TerminalVisibilityType,
  toggleEffectiveTerminalTypeInGroup,
} from '../terminal-visibility';
import { wireTopLayerOverlays } from '../top-layer-overlay';
import { data } from './dom';
import { type Control, type Project } from './types';

/** Live application bindings used by this handler group. */
export interface TerminalInteractionsDependencies {
  readonly terminalDrawerBounds: Signal<{ width: number; height: number }>;
  readonly terminalDashboardSize: Signal<{ width: number; height: number }>;
  readonly terminalGridFullscreen: Signal<boolean>;
  readonly terminalDrawerFitHigh: Signal<number>;
  readonly terminalFitAcross: Signal<number>;
  readonly terminalFitHigh: Signal<number>;
  readonly terminalFitPhone: Signal<number>;
  terminalPreviewClickTimer: number | undefined;
  readonly terminalSession: (key?: string) => TerminalDashboardSession | undefined;
  readonly clearTerminalHalt: (key: string) => Promise<void>;
  /** HS2-H3S7YQ: choose or clear a terminal as its project's feedback-primary session. */
  readonly setFeedbackPrimary: (key: string, command: FeedbackPrimaryCommand) => Promise<void>;
  pendingTerminalFocus: TerminalFocusRequest | undefined;
  readonly magnifiedTerminalKey: Signal<string | undefined>;
  readonly openTerminalInProject: (key: string) => void;
  readonly terminalContextMenu: Signal<{ key: string; x: number; y: number } | undefined>;
  readonly terminalVisibilityScopeFor: (target: Element) => string;
  readonly terminalVisibility: Signal<TerminalVisibilityState>;
  readonly persistTerminalVisibility: (next: TerminalVisibilityState) => void;
  readonly terminalVisibilityContextMenu: Signal<{ id: string; x: number; y: number } | undefined>;
  readonly terminalVisibilityDialogScope: Signal<string | undefined>;
  readonly terminalVisibilityNamePrompt: Signal<TerminalVisibilityNamePrompt | undefined>;
  readonly terminalKeysForVisibilityDialog: () => string[];
  /** The item types a scope's visibility dialog offers (HS2-4TKASN). */
  readonly terminalAvailableTypes: (scope: string) => readonly TerminalVisibilityType[];
  readonly terminalVisibleTypes: (scope: string) => readonly TerminalVisibilityType[];
  readonly openGridAIChat: (projectId: string, chatId: string) => void;
  readonly setTerminalDrawerVisible: (visible: boolean, refresh?: boolean) => void;
  readonly terminalDrawerVisible: Signal<boolean>;
  readonly toggleTerminalDrawerMaximized: () => void;
  readonly selectDrawerItem: (id: string) => void;
  readonly retryFailedDrawerTab: (id: string) => void;
  readonly closeFailedDrawerTab: (id: string) => void;
  readonly enterMobileTerminalFocus: (terminalId: string, search?: boolean) => void;
  readonly exitMobileTerminalFocus: () => void;
  /** Open or close the full-screen terminal's search bar (HS2-7HPXCY). */
  readonly setMobileTerminalSearch: (open: boolean) => void;
  /** The phone full-screen terminal state, whose `search` says the search bar is open (HS2-7HPXCY). */
  readonly mobileTerminalFocus: Signal<MobileTerminalFocusState>;
  readonly cycleMobileTerminalColumns: () => void;
  /** Phone key-bar sticky modifiers and Fn-row state (HS2-CKS78M). */
  readonly terminalModifiers: Signal<TerminalModifiers>;
  readonly terminalFunctionRow: Signal<boolean>;
  /** Phone terminal Copy and Paste-fallback sheets (HS2-FRB545). */
  readonly terminalCopy: Signal<TerminalCopyState | undefined>;
  readonly terminalPaste: Signal<TerminalPasteState | undefined>;
  /** Long-press terminal edit menu (HS2-KKP8YJ). */
  readonly terminalEditMenu: Signal<TerminalEditMenuState | undefined>;
  readonly showToast: (message: string) => void;
  readonly focusDrawerTab: (projectId: string, id: string) => void;
  readonly createProjectTerminal: (selection?: AiToolDefaults) => Promise<void>;
  readonly aiLaunchConfiguration: (
    kind: 'ai-shell' | 'ai-chat',
    customize: boolean,
    provider?: string,
  ) => Promise<AiToolDefaults | undefined>;
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
  readonly minimizeProjectTab: (id: string) => void;
  readonly restoreMinimizedProject: (id: string) => void;
  readonly minimizedProjectIds: Signal<string[]>;
  readonly projectCloseDialog: Signal<ProjectCloseDialogState | undefined>;
  readonly restoreBorrowedProjectCloseTerminal: (state: ProjectCloseDialogState | undefined) => void;
  readonly cancelProjectClose: () => void;
  readonly confirmProjectClose: () => void;
  readonly closeAllProjectResources: () => Promise<void>;
  readonly closeTerminalIds: (ids: readonly string[]) => Promise<void>;
  readonly closeDrawerAIChat: (id: string) => void;
  readonly appTabContextMenu: Signal<AppTabMenuState | undefined>;
  readonly projects: Signal<Project[]>;
  readonly projectRestoreFailures: Signal<ProjectRestoreFailure[]>;
  readonly currentDrawerTabIds: (projectId: string) => string[];
  readonly project: () => Project | undefined;
  readonly terminalGroups: Signal<TerminalDashboardGroup[]>;
  readonly terminalRename: Signal<TerminalRenameTarget | undefined>;
  readonly closeDrawerTabIds: (ids: readonly string[]) => Promise<void>;
  readonly saveTerminalName: (projectId: string, terminalId: string, name: string) => void;
  /** HS2-6MNMVB: a person takes over (and optionally pauses) a terminal's orchestrator session. */
  readonly takeOverHostedSession: (terminalId: string, pause: boolean) => Promise<void>;
  /** HS2-KR28FS: a person hands a terminal's orchestrator session back. */
  readonly handBackHostedSession: (terminalId: string) => Promise<void>;
  /** Clear a terminal's saved name so its tab shows the derived default again (HS2-2Q7KTX). */
  readonly resetTerminalName: (projectId: string, terminalId: string) => void;
  /** The terminal drawer's Kerf-managed search field model (HS2-TWZSZF). */
  readonly terminalSearchModel: SearchFieldModel;
  /** Whether the drawer's terminal search field is expanded (HS2-TWZSZF). */
  readonly terminalSearchOpen: Signal<boolean>;
  /** The searched terminal's match count and active match (HS2-M5QPED); undefined while nothing is searched. */
  readonly terminalSearchResults: Signal<TerminalSearchResults | undefined>;
  /** The drawer's selected item; a change moves the search to the newly selected terminal. */
  readonly terminalDrawerSelected: Signal<string>;
}

/** Register this group only when the application wiring owner invokes it. */
export function wireTerminalInteractions(dependencies: TerminalInteractionsDependencies) {
  const lifetime = createScope();
  let terminalRenameSession = 0;
  const {
    terminalDrawerBounds,
    terminalDashboardSize,
    terminalGridFullscreen,
    terminalDrawerFitHigh,
    terminalFitAcross,
    terminalFitHigh,
    terminalFitPhone,
    terminalSession,
    clearTerminalHalt,
    setFeedbackPrimary,
    magnifiedTerminalKey,
    openTerminalInProject,
    terminalContextMenu,
    terminalVisibilityScopeFor,
    terminalVisibility,
    persistTerminalVisibility,
    terminalVisibilityContextMenu,
    terminalVisibilityDialogScope,
    terminalVisibilityNamePrompt,
    terminalKeysForVisibilityDialog,
    terminalAvailableTypes,
    terminalVisibleTypes,
    openGridAIChat,
    setTerminalDrawerVisible,
    terminalDrawerVisible,
    toggleTerminalDrawerMaximized,
    selectDrawerItem,
    retryFailedDrawerTab,
    closeFailedDrawerTab,
    enterMobileTerminalFocus,
    exitMobileTerminalFocus,
    setMobileTerminalSearch,
    mobileTerminalFocus,
    cycleMobileTerminalColumns,
    terminalModifiers,
    terminalFunctionRow,
    terminalCopy,
    terminalPaste,
    terminalEditMenu,
    showToast,
    focusDrawerTab,
    createProjectTerminal,
    aiLaunchConfiguration,
    createDrawerAIChat,
    openSavedConversation,
    requestProjectClose,
    minimizeProjectTab,
    restoreMinimizedProject,
    minimizedProjectIds,
    projectCloseDialog,
    restoreBorrowedProjectCloseTerminal,
    cancelProjectClose,
    confirmProjectClose,
    closeAllProjectResources,
    closeTerminalIds,
    closeDrawerAIChat,
    appTabContextMenu,
    projects,
    projectRestoreFailures,
    currentDrawerTabIds,
    project,
    terminalGroups,
    terminalRename,
    closeDrawerTabIds,
    saveTerminalName,
    resetTerminalName,
    terminalSearchModel,
    takeOverHostedSession,
    handBackHostedSession,
    terminalSearchOpen,
    terminalSearchResults,
    terminalDrawerSelected,
  } = dependencies;
  // The magnified terminal (HS2-Z9PQSC) and the shell's permission popup (HS2-ZESCM2) are manual
  // popovers; keep them in the top layer as Kerf renders them.
  wireTopLayerOverlays(document.body);
  lifetime.add(
    delegate(document.body, 'focusin', '.terminal-session:not([hidden]) .xterm-helper-textarea', (_event, target) => {
      const viewport = target.closest<HTMLElement>('[data-terminal-id]');
      if (!viewport?.closest('[data-component="terminal-drawer"][data-mode="dedicated"]')) return;
      enterMobileTerminalFocus(viewport.dataset.terminalId!);
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', TERMINALS_ACTIONS.exitTerminalFocusMode.selector, () => {
      terminalModifiers.value = NO_TERMINAL_MODIFIERS;
      terminalFunctionRow.value = false;
      const current = project(),
        drawer = document.querySelector<HTMLElement>('[data-component="terminal-drawer"]'),
        terminalId = drawer?.querySelector<HTMLElement>('.terminal-session:not([hidden]) [data-terminal-id]')?.dataset
          .terminalId,
        searching = mobileTerminalFocus.peek().search === true;
      exitMobileTerminalFocus();
      if (searching) {
        // Exit leaves full screen, not the search (HS2-7HPXCY): a query keeps its highlights in the
        // drawer, an empty one closes, and focus returns to the rail's Search.
        if (!terminalSearchModel.state.peek().query) terminalSearchOpen.value = false;
        requestAnimationFrame(() => {
          document
            .querySelector<HTMLElement>(
              `[data-component="terminal-drawer"] ${TERMINALS_ACTIONS.openTerminalFocusSearch.selector}`,
            )
            ?.focus();
        });
        return;
      }
      if (current && drawer?.dataset.mode === 'dedicated' && terminalId) focusDrawerTab(current.id, terminalId);
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', TERMINALS_ACTIONS.toggleTerminalGridFullscreen.selector, () => {
      terminalGridFullscreen.value = !terminalGridFullscreen.value;
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', TERMINALS_ACTIONS.zoomTerminalGrid.selector, (_event, target) => {
      const drawer = Boolean(target.closest('[data-component="terminal-drawer"]')),
        bounds = drawer ? terminalDrawerBounds.value : terminalDashboardSize.value,
        phone = !drawer && window.innerWidth <= TERMINAL_GRID_PHONE_BREAKPOINT,
        basis = drawer
          ? 'high'
          : terminalGridBasis(bounds.height, phone ? TERMINAL_GRID_PHONE_BREAKPOINT : Number.POSITIVE_INFINITY),
        direction = data(target).zoomDirection as 'in' | 'out';
      if (drawer) {
        terminalDrawerFitHigh.value = adjustTerminalFit(terminalDrawerFitHigh.value, basis, direction);
        localStorage.setItem('hotsheet.terminals.drawer-fit-high', String(terminalDrawerFitHigh.value));
      } else if (phone) {
        terminalFitPhone.value = Math.min(
          TERMINAL_GRID_MAX_PHONE,
          adjustTerminalFit(terminalFitPhone.value, 'across', direction),
        );
        localStorage.setItem('hotsheet.terminals.fit-phone', String(terminalFitPhone.value));
      } else if (basis === 'across') {
        terminalFitAcross.value = adjustTerminalFit(terminalFitAcross.value, basis, direction);
        localStorage.setItem('hotsheet.terminals.fit-across', String(terminalFitAcross.value));
      } else {
        terminalFitHigh.value = adjustTerminalFit(terminalFitHigh.value, basis, direction);
        localStorage.setItem('hotsheet.terminals.fit-high', String(terminalFitHigh.value));
      }
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', TERMINALS_ACTIONS.previewTerminal.selector, (event, target) => {
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
    }),
  );
  lifetime.add(
    delegate(document.body, 'keydown', TERMINALS_ACTIONS.previewTerminal.selector, (event, target) => {
      const keyboard = event as KeyboardEvent;
      if (keyboard.key !== 'Enter' && keyboard.key !== ' ') return;
      event.preventDefault();
      const key = data(target).terminalKey,
        session = terminalSession(key);
      if (!session) return;
      dependencies.pendingTerminalFocus = { projectId: session.projectId, terminalId: session.id };
      magnifiedTerminalKey.value = key;
    }),
  );
  lifetime.add(
    delegate(document.body, 'dblclick', TERMINALS_TARGETS.terminalTile.selector, (event, target) => {
      if (data(target).magnified === 'true') return;
      event.preventDefault();
      if (dependencies.terminalPreviewClickTimer !== undefined) {
        window.clearTimeout(dependencies.terminalPreviewClickTimer);
        dependencies.terminalPreviewClickTimer = undefined;
      }
      openTerminalInProject(data(target).terminalKey!);
    }),
  );
  lifetime.add(
    delegate(document.body, 'dblclick', '.terminal-dashboard__magnified .terminal-tile__footer', (event, target) => {
      if ((event.target as Element).closest('button')) return;
      event.preventDefault();
      openTerminalInProject(data(target.closest('[data-component="terminal-tile"]')!).terminalKey!);
    }),
  );
  lifetime.add(
    wireTerminalDashboardMenu(document.body, {
      open: (menu) => {
        terminalContextMenu.value = menu;
      },
      clearHalt: (key) => {
        terminalContextMenu.value = undefined;
        if (key) void clearTerminalHalt(key);
      },
      feedbackPrimary: (key, command) => {
        terminalContextMenu.value = undefined;
        if (key) void setFeedbackPrimary(key, command);
      },
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', TERMINALS_ACTIONS.dismissMagnifiedTerminal.selector, (event, target) => {
      // Only the desktop scrim dismisses on click-away. The phone overlay is a full-bleed terminal
      // whose blackout backdrop and safe-area insets are not a scrim, and it has an explicit Close
      // (HS2-SB1FSQ).
      if (data(target).mobile === 'true') return;
      if (event.target === target) magnifiedTerminalKey.value = undefined;
    }),
  );
  // Leaving a phone terminal clears its key-bar state, so a locked modifier never carries over.
  const resetKeyBar = () => {
    terminalModifiers.value = NO_TERMINAL_MODIFIERS;
    terminalFunctionRow.value = false;
  };
  lifetime.add(
    delegate(document.body, 'click', TERMINALS_ACTIONS.closeMagnifiedTerminal.selector, () => {
      magnifiedTerminalKey.value = undefined;
      resetKeyBar();
    }),
  );
  // Phone key bar (HS2-CKS78M). Its buttons never take focus: cancelling the mousedown default (also
  // dispatched for touch taps) keeps the terminal's textarea focused so the soft keyboard stays up, and
  // each action restores that focus if something else took it.
  lifetime.add(
    delegate(document.body, 'mousedown', TERMINALS_TARGETS.terminalKeyBar.selector, (event) => {
      event.preventDefault();
    }),
  );
  const keyBarViewport = (target: Element) =>
    target
      .closest('[data-component="terminal-drawer"], [data-component="terminal-tile"]')
      ?.querySelector<HTMLElement>(
        '.terminal-session:not([hidden]) [data-display-mode="interactive"], .terminal-tile__viewport-frame [data-display-mode="interactive"]',
      );
  const keepTerminalFocus = (viewport: HTMLElement | null | undefined) => {
    const input = viewport?.querySelector<HTMLElement>('.xterm-helper-textarea');
    if (input && document.activeElement !== input) input.focus({ preventScroll: true });
  };
  lifetime.add(
    wireTerminalKeyBar(document.body, {
      toggleModifier: (modifier, target) => {
        terminalModifiers.value = toggleTerminalModifier(terminalModifiers.value, modifier);
        keepTerminalFocus(keyBarViewport(target));
      },
      toggleFunctionRow: (target) => {
        terminalFunctionRow.value = !terminalFunctionRow.value;
        keepTerminalFocus(keyBarViewport(target));
      },
      sendKey: (key, target) => {
        const viewport = keyBarViewport(target);
        if (viewport) viewport.dispatchEvent(new CustomEvent(TERMINAL_KEY_EVENT, { detail: { key } }));
        keepTerminalFocus(viewport);
      },
    }),
  );
  // Phone terminal clipboard (HS2-FRB545). The sheet keeps the viewport it was opened from, so a paste
  // lands in that terminal even if the drawer selection changed underneath the dialog.
  let clipboardViewport: HTMLElement | undefined,
    editMenuTarget: { viewport: HTMLElement; selection?: string } | undefined,
    editMenuFocus: HTMLElement | undefined,
    refocusAfterPaste: HTMLElement | undefined,
    clipboardGeneration = 0;
  const viewportTitle = (viewport: HTMLElement) =>
    viewport.getAttribute('aria-label')?.replace(/ interactive terminal$/, '') || 'the terminal';
  const sheetField = (name: 'terminal-copy-text' | 'terminal-paste-text') =>
    document.querySelector<HTMLTextAreaElement>(`textarea[name="${name}"]`);
  const afterSheetRender = (callback: () => void) => {
    window.requestAnimationFrame(() => {
      window.requestAnimationFrame(callback);
    });
  };
  const closeCopySheet = () => {
    const current = terminalCopy.peek();
    if (current?.open) terminalCopy.value = { ...current, open: false };
  };
  const closePasteSheet = () => {
    const current = terminalPaste.peek();
    if (current?.open) terminalPaste.value = { ...current, open: false };
  };
  // The terminal edit menu acts on the terminal it was opened over; toolbar, pill, and key-bar
  // buttons act on the terminal of the drawer or tile that holds them.
  const clipboardTarget = (target: Element) => {
    if (!target.closest(TERMINALS_TARGETS.terminalEditMenu.selector)) return keyBarViewport(target);
    const viewport = editMenuTarget?.viewport;
    editMenuTarget = undefined;
    terminalEditMenu.value = undefined;
    return viewport?.isConnected ? viewport : undefined;
  };
  const openEditMenu = (viewport: HTMLElement, x: number, y: number, selection?: string) => {
    // The pointer and focus move into the popup. Keep the selection from the terminal at open time.
    editMenuTarget = { viewport, selection };
    // Kerf's menu takes focus while open (beta.106); a terminal that held the keyboard gets it back.
    const active = document.activeElement;
    editMenuFocus = active instanceof HTMLElement && viewport.contains(active) ? active : undefined;
    terminalEditMenu.value = {
      ...viewportSafeContextMenuPosition(x, y, window.innerWidth, window.innerHeight, {
        width: 192,
        height: selection ? 144 : 96,
      }),
      selection: Boolean(selection),
    };
  };
  lifetime.add(
    delegate(document.body, TERMINAL_EDIT_MENU_EVENT, TERMINALS_TARGETS.terminalViewport.selector, (event, target) => {
      const { x, y, selection } = (event as CustomEvent<TerminalEditMenuDetail>).detail;
      const viewport = target as HTMLElement;
      openEditMenu(viewport, x, y, selection ? readTerminalViewportSelection(viewport) : undefined);
    }),
  );
  // Desktop right-click on a selection uses the same local clipboard as the touch menu (HS2-RJRRSF).
  // An unselected terminal still gets the browser's normal context menu.
  lifetime.add(
    delegateCapture(document.body, 'contextmenu', TERMINALS_TARGETS.terminalViewport.selector, (event, target) => {
      const viewport = target as HTMLElement;
      if (!(event instanceof MouseEvent) || event.button !== 2) return;
      const selection = readTerminalViewportSelection(viewport);
      if (!selection) return;
      event.preventDefault();
      event.stopPropagation();
      openEditMenu(viewport, event.clientX, event.clientY, selection);
    }),
  );
  // Once the edit menu closes (an item, Escape, or an outside press), return the keyboard to the
  // terminal that held it, unless the user has since moved focus elsewhere (HS2-BH7NZ4).
  const restoreEditMenuFocus = (event: Event) => {
    if (closedPopupMenuRoot(event)?.dataset.contextMenu !== 'terminal-edit') return;
    if (!terminalEditMenu.peek()) editMenuTarget = undefined;
    const focus = editMenuFocus;
    editMenuFocus = undefined;
    if (!focus) return;
    window.requestAnimationFrame(() => {
      const active = document.activeElement;
      const lost =
        !active || active === document.body || !active.isConnected || Boolean(active.closest('.kui-popup-menu'));
      if (focus.isConnected && lost) focus.focus({ preventScroll: true });
    });
  };
  document.addEventListener('toggle', restoreEditMenuFocus, { capture: true, signal: lifetime.signal });
  const appleClipboardPlatform = isAppleShortcutPlatform();
  const copyTerminalSelection = (text: string) => {
    void writeClipboardText(text, navigator.clipboard as Clipboard | undefined, () => copyWithSelection(text)).then(
      (copied) => {
        showToast(copied ? terminalCopyMessage(text, true) : 'Could not copy the selection.');
      },
    );
  };
  // Escape dismisses the open edit menu even while a focused terminal holds the keyboard: xterm's
  // helper textarea handles Escape and stops it, so the document-level dismiss never sees it. Capture
  // it first, and do not also send ESC to the shell (HS2-B06X7Y).
  lifetime.add(
    delegateCapture(document.body, 'keydown', TERMINALS_TARGETS.terminalViewport.selector, (event, target) => {
      const keyboard = event as KeyboardEvent;
      if (terminalSelectionCopyKey(keyboard, appleClipboardPlatform)) {
        const text = readTerminalViewportSelection(target as HTMLElement);
        if (!text) return;
        keyboard.preventDefault();
        keyboard.stopPropagation();
        copyTerminalSelection(text);
        return;
      }
      if (!terminalEditMenuDismissKey(keyboard, Boolean(terminalEditMenu.peek()))) return;
      keyboard.preventDefault();
      keyboard.stopPropagation();
      editMenuTarget = undefined;
      terminalEditMenu.value = undefined;
    }),
  );
  // Clear Terminal (HS2-N2AG8V): the rail button clears the selected terminal, and Command-K clears the
  // focused one. The key is captured before xterm, so it never reaches the shell, and the global
  // dispatcher skips Open Search for the same chord inside a terminal (`terminalClearKeyTarget`).
  // The clear is then shared through the project's server (HS2-HA0Z3A), so a reload or another client
  // no longer replays the cleared output; if the server cannot share it, the local clear stands.
  const clearTerminal = (viewport: HTMLElement | null | undefined) => {
    const identity = viewport ? terminalViewportIdentity(viewport) : undefined;
    if (!identity) return;
    void clearTerminalEverywhere(document, identity, ({ projectId, terminalId }) => {
      const target = projects.peek().find((item) => item.id === projectId);
      return target ? new Api(target.apiPath).clearTerminal(terminalId) : Promise.reject(new Error('unknown project'));
    });
  };
  // Pressing Clear leaves the keyboard in the terminal, as the key bar does, so typing continues.
  lifetime.add(
    delegate(document.body, 'mousedown', TERMINALS_ACTIONS.clearTerminal.selector, (event) => {
      event.preventDefault();
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', TERMINALS_ACTIONS.clearTerminal.selector, (_event, target) => {
      const viewport = keyBarViewport(target);
      clearTerminal(viewport);
      keepTerminalFocus(viewport);
    }),
  );
  lifetime.add(
    delegateCapture(document.body, 'keydown', TERMINALS_TARGETS.terminalViewport.selector, (event) => {
      const viewport = terminalClearKeyTarget(event as KeyboardEvent);
      if (!viewport) return;
      event.preventDefault();
      event.stopPropagation();
      clearTerminal(viewport);
    }),
  );
  // Terminal search (HS2-TWZSZF): typing in the drawer's search field searches the selected terminal
  // incrementally, and Enter or Shift-Enter moves to the next or previous match. HS2-M5QPED: every
  // match is highlighted and counted; emptying or closing the field clears the highlights and the
  // count, and selecting another terminal moves the search there. Kerf owns the field's text, clear
  // button, and collapse.
  const searchViewport = () =>
    document.querySelector<HTMLElement>(
      '[data-component="terminal-drawer"] .terminal-session:not([hidden]) [data-display-mode="interactive"]',
    );
  const terminalSearch = createTerminalSearchSync({
    viewport: searchViewport,
    query: () => (terminalSearchOpen.peek() ? terminalSearchModel.state.peek().query : ''),
    publish: (results) => {
      terminalSearchResults.value = results;
    },
  });
  lifetime.add(
    effect(() => {
      // Track the query, the field's open state, and the selected terminal; search once the render the
      // change caused has placed the selected terminal's viewport.
      void terminalSearchModel.state.value.query;
      void terminalSearchOpen.value;
      void terminalDrawerSelected.value;
      queueMicrotask(() => {
        terminalSearch.sync();
      });
    }),
  );
  lifetime.add(
    delegate(
      document.body,
      TERMINAL_SEARCH_RESULTS_EVENT,
      TERMINALS_TARGETS.terminalViewport.selector,
      (event, target) => {
        terminalSearch.receive(target, (event as CustomEvent<TerminalSearchResults>).detail);
      },
    ),
  );
  lifetime.add(
    delegate(
      document.body,
      TERMINAL_SEARCH_READY_EVENT,
      TERMINALS_TARGETS.terminalViewport.selector,
      (_event, target) => {
        terminalSearch.ready(target);
      },
    ),
  );
  lifetime.add(
    delegateCapture(document.body, 'keydown', TERMINALS_TARGETS.terminalSearchEditor.selector, (event) => {
      const keyboard = event as KeyboardEvent;
      if (keyboard.key !== 'Enter' || keyboard.isComposing || keyboard.altKey || keyboard.ctrlKey || keyboard.metaKey)
        return;
      // The terminal search has no submit: Enter steps through matches instead of reaching Kerf's submit.
      keyboard.preventDefault();
      keyboard.stopPropagation();
      terminalSearch.step(keyboard.shiftKey ? 'previous' : 'next');
    }),
  );
  // The match count steps to the next match (Shift-click to the previous one) and leaves the keyboard
  // in the search field, so typing refines the query (HS2-M5QPED).
  lifetime.add(
    delegate(document.body, 'mousedown', TERMINALS_ACTIONS.nextTerminalSearchMatch.selector, (event) => {
      event.preventDefault();
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', TERMINALS_ACTIONS.nextTerminalSearchMatch.selector, (event) => {
      terminalSearch.step((event as MouseEvent).shiftKey ? 'previous' : 'next');
    }),
  );
  // The phone search bar's Previous Match keeps the keyboard in the field the same way (HS2-7HPXCY).
  lifetime.add(
    delegate(document.body, 'mousedown', TERMINALS_ACTIONS.previousTerminalSearchMatch.selector, (event) => {
      event.preventDefault();
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', TERMINALS_ACTIONS.previousTerminalSearchMatch.selector, () => {
      terminalSearch.step('previous');
    }),
  );
  // Phone Search (HS2-7HPXCY): from the drawer rail it opens the selected terminal full screen with its
  // search bar; from the full-screen controls it opens the bar there. The field takes focus once the
  // bar renders, keeping any query the search already has.
  lifetime.add(
    delegate(document.body, 'click', TERMINALS_ACTIONS.openTerminalFocusSearch.selector, () => {
      const terminalId = searchViewport()?.closest<HTMLElement>('[data-terminal-id]')?.dataset.terminalId;
      if (!terminalId) return;
      terminalSearchOpen.value = true;
      if (mobileTerminalFocus.peek().active) setMobileTerminalSearch(true);
      else enterMobileTerminalFocus(terminalId, true);
      requestAnimationFrame(() => {
        const editor = document.querySelector<HTMLElement>(
          `[data-component="terminal-focus-search"] ${TERMINALS_TARGETS.terminalSearchEditor.selector}`,
        );
        // A kept query continues where it ends.
        if (editor) placeSearchFieldCaret(editor);
      });
    }),
  );
  // Copy from the edit menu sends the viewport's selection straight to this device's clipboard.
  lifetime.add(
    wireTerminalClipboardActions(document.body, {
      copySelection: (target) => {
        const text = editMenuTarget?.selection;
        if (clipboardTarget(target) && text) copyTerminalSelection(text);
      },
      copyText: (target) => {
        const viewport = clipboardTarget(target);
        if (!viewport) return;
        clipboardViewport = viewport;
        terminalCopy.value = {
          open: true,
          title: viewportTitle(viewport),
          generation: ++clipboardGeneration,
          text: readTerminalViewportText(viewport) ?? '',
        };
        // Open on the newest output, where the text a user wants usually is.
        afterSheetRender(() => {
          const field = sheetField('terminal-copy-text');
          if (field) field.scrollTop = field.scrollHeight;
        });
      },
      paste: (target) => {
        const viewport = clipboardTarget(target);
        if (!viewport) return;
        clipboardViewport = viewport;
        void readClipboardText(navigator.clipboard as Clipboard | undefined).then((result) => {
          if (result.status === 'ok') {
            if (result.text) pasteIntoTerminalViewport(viewport, result.text);
            else showToast('The clipboard is empty');
            keepTerminalFocus(viewport);
            return;
          }
          terminalPaste.value = {
            open: true,
            title: viewportTitle(viewport),
            generation: ++clipboardGeneration,
            reason: result.status,
          };
          afterSheetRender(() => sheetField('terminal-paste-text')?.focus());
        });
      },
    }),
  );
  lifetime.add(
    wireTerminalClipboardSheets(document.body, {
      confirmCopy: (field) => {
        const { text, selection } = terminalCopySelection(field.value, field.selectionStart, field.selectionEnd),
          start = selection ? field.selectionStart : 0,
          end = selection ? field.selectionEnd : field.value.length;
        void writeClipboardText(text, navigator.clipboard as Clipboard | undefined, () => {
          field.focus({ preventScroll: true });
          field.setSelectionRange(start, end);
          // Compatibility boundary (as in copy-text.ts): plain-HTTP LAN origins have no async Clipboard
          // API. The shared off-screen copyWithSelection helper cannot select text outside this modal
          // dialog (the rest of the page is inert), so select within the sheet's own field instead.
          // eslint-disable-next-line @typescript-eslint/no-deprecated
          return document.execCommand('copy');
        }).then((copied) => {
          if (!copied) {
            showToast('Could not copy. Touch and hold the text to copy it.');
            return;
          }
          showToast(terminalCopyMessage(text, selection));
          closeCopySheet();
        });
      },
      closeCopy: closeCopySheet,
      submitPaste: (text) => {
        const viewport = clipboardViewport;
        closePasteSheet();
        if (!viewport?.isConnected) return;
        if (text) pasteIntoTerminalViewport(viewport, text);
        // The dialog hands focus back to its opener once hidden; return it to the terminal instead.
        refocusAfterPaste = viewport;
      },
      closePaste: closePasteSheet,
    }),
  );
  lifetime.add(
    // The native `close` event does not bubble, so it is delegated in the capture phase.
    delegateCapture(document.body, 'close', TERMINALS_TARGETS.terminalPasteDialog.selector, (event, target) => {
      if (event.target !== target) return;
      const viewport = refocusAfterPaste;
      refocusAfterPaste = undefined;
      // The closing dialog returns focus to its opener first; run after it.
      if (viewport?.isConnected)
        window.setTimeout(() => {
          keepTerminalFocus(viewport);
        });
    }),
  );
  lifetime.add(
    wireTerminalTextSizeControl(document.body, () => {
      cycleMobileTerminalColumns();
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', TERMINALS_ACTIONS.hideDashboardTerminal.selector, (_event, target) => {
      const key = data(target).terminalKey ?? data(target).itemId,
        scope = terminalVisibilityScopeFor(target),
        active = activeTerminalVisibilityGroup(terminalVisibility.value, scope);
      if (key) persistTerminalVisibility(setTerminalVisibleInGroup(terminalVisibility.value, active.id, key, false));
      terminalContextMenu.value = undefined;
      if (magnifiedTerminalKey.value === key) magnifiedTerminalKey.value = undefined;
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', TERMINALS_ACTIONS.openTerminalVisibility.selector, (event, target) => {
      event.stopImmediatePropagation();
      terminalVisibilityContextMenu.value = undefined;
      terminalVisibilityDialogScope.value = terminalVisibilityScopeFor(target);
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', TERMINALS_ACTIONS.toggleTerminalVisibilityType.selector, (_event, target) => {
      const scope = terminalVisibilityDialogScope.value,
        type = data(target).visibilityType as TerminalVisibilityType | undefined;
      const available = scope ? terminalAvailableTypes(scope) : [];
      if (!scope || !type || !available.includes(type)) return;
      // Toggle from what the workspace applies, so a fallback type releases cleanly (HS2-4TKASN).
      const active = activeTerminalVisibilityGroup(terminalVisibility.value, scope),
        visible = !terminalVisibleTypes(scope).includes(type);
      persistTerminalVisibility(
        toggleEffectiveTerminalTypeInGroup(terminalVisibility.value, active.id, type, available),
      );
      const magnified = terminalSession(magnifiedTerminalKey.value);
      if (!visible && magnified && (magnified.kind === 'ai' ? 'ai' : 'shell') === type)
        magnifiedTerminalKey.value = undefined;
    }),
  );
  lifetime.add(
    delegate(document.body, 'kui-request-close', '[data-terminal-visibility-dialog]', (event, target) => {
      if (event.target !== target) return;
      terminalVisibilityContextMenu.value = undefined;
      terminalVisibilityNamePrompt.value = undefined;
      terminalVisibilityDialogScope.value = undefined;
    }),
  );
  lifetime.add(
    delegate(document.body, 'change', TERMINALS_TARGETS.terminalVisibilityGroupField.selector, (_event, target) => {
      const scope = terminalVisibilityScopeFor(target),
        id = (target as Control).value;
      persistTerminalVisibility(selectTerminalVisibilityGroup(terminalVisibility.value, scope, id));
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', TERMINALS_ACTIONS.selectTerminalVisibilityTab.selector, (_event, target) => {
      const scope = terminalVisibilityDialogScope.value,
        id = data(target).tabId;
      terminalVisibilityContextMenu.value = undefined;
      if (scope && id) persistTerminalVisibility(selectTerminalVisibilityGroup(terminalVisibility.value, scope, id));
    }),
  );
  lifetime.add(
    delegate(document.body, 'contextmenu', '[data-visibility-group-id]', (event, target) => {
      const id = data(target).visibilityGroupId;
      if (!id || id === 'default') return;
      event.preventDefault();
      const pointer = event as MouseEvent;
      // Kerf's context PopupMenu flips and clamps itself at the pointer, so the raw point is the anchor.
      terminalVisibilityContextMenu.value = { id, x: pointer.clientX, y: pointer.clientY };
      revealContextPopupMenu('terminal-visibility-group');
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', TERMINALS_ACTIONS.addTerminalVisibilityGroup.selector, () => {
      terminalVisibilityContextMenu.value = undefined;
      terminalVisibilityNamePrompt.value = { mode: 'add', value: '' };
      requestAnimationFrame(() =>
        document
          .querySelector<HTMLElement>('[data-terminal-visibility-name-dialog] [name="terminal-visibility-group-name"]')
          ?.focus(),
      );
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', TERMINALS_ACTIONS.renameTerminalVisibilityGroup.selector, () => {
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
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', TERMINALS_ACTIONS.removeTerminalVisibilityGroup.selector, () => {
      const id = terminalVisibilityContextMenu.value?.id;
      terminalVisibilityContextMenu.value = undefined;
      if (id) persistTerminalVisibility(removeTerminalVisibilityGroup(terminalVisibility.value, id));
    }),
  );
  lifetime.add(
    delegate(document.body, 'submit', TERMINALS_ACTIONS.submitTerminalVisibilityName.selector, (event, target) => {
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
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', TERMINALS_ACTIONS.cancelTerminalVisibilityName.selector, () => {
      terminalVisibilityNamePrompt.value = undefined;
    }),
  );
  lifetime.add(
    delegate(document.body, 'kui-request-close', '[data-terminal-visibility-name-dialog]', () => {
      terminalVisibilityNamePrompt.value = undefined;
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', TERMINALS_ACTIONS.toggleTerminalVisibility.selector, (_event, target) => {
      const scope = terminalVisibilityDialogScope.value,
        key = data(target).itemId;
      if (!scope || !key) return;
      const active = activeTerminalVisibilityGroup(terminalVisibility.value, scope),
        visible = active.hiddenKeys.includes(key);
      persistTerminalVisibility(setTerminalVisibleInGroup(terminalVisibility.value, active.id, key, visible));
    }),
  );
  lifetime.add(
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
          setAllTerminalsVisibleInGroup(
            terminalVisibility.value,
            active.id,
            terminalKeysForVisibilityDialog(),
            visible,
          ),
        );
      },
    ),
  );
  lifetime.add(
    delegate(document.body, 'click', TERMINALS_ACTIONS.openTerminalProject.selector, (_event, target) => {
      openTerminalInProject(data(target).terminalKey ?? data(target).itemId!);
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', TERMINALS_ACTIONS.openGridAiChat.selector, (_event, target) => {
      openGridAIChat(data(target).projectId!, data(target).chatId!);
    }),
  );
  lifetime.add(
    delegate(document.body, 'keydown', TERMINALS_TARGETS.workspaceChatTile.selector, (event, target) => {
      const keyboard = event as KeyboardEvent;
      if (keyboard.key !== 'Enter' && keyboard.key !== ' ') return;
      event.preventDefault();
      openGridAIChat(data(target).projectId!, data(target).chatId!);
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', TERMINALS_ACTIONS.toggleTerminalDrawer.selector, () => {
      setTerminalDrawerVisible(!terminalDrawerVisible.value);
    }),
  );
  lifetime.add(
    delegate(document.body, 'dblclick', TERMINALS_ACTIONS.toggleTerminalDrawerMaximize.selector, (event) => {
      // Only the rail's empty space maximizes. A double-click inside a control, including the
      // terminal search field's contenteditable editor and its group's padding, selects text there
      // instead (HS2-SW2YAZ).
      if (
        (event.target as Element).closest(
          'button, input, textarea, select, a, wa-button, .kui-popup-menu, [data-tab-kind="terminal"], ' +
            '[contenteditable="true"], [role="searchbox"], [data-component="search-field"], ' +
            '[data-component="toolbar-control-group"]',
        )
      )
        return;
      toggleTerminalDrawerMaximized();
    }),
  );
  lifetime.add(
    delegate(document.body, 'dblclick', '[data-tab-kind="terminal"], [data-action="select-drawer-item"]', (event) => {
      if ((event.target as Element).closest('[data-tab-kind="ai-chat"]')) return;
      event.stopPropagation();
      toggleTerminalDrawerMaximized();
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', TERMINALS_ACTIONS.selectDrawerItem.selector, (_event, target) => {
      const tab = target.closest<HTMLElement>('[data-tab-kind]'),
        // A pending tab (HS2-AQ40G7) has no tab kind, so the app-tab menus and Delete skip it.
        pending = target.closest<HTMLElement>('[data-pending-tab-id]');
      selectDrawerItem(
        tab?.dataset.terminalId ||
          tab?.dataset.chatId ||
          pending?.dataset.pendingTabId ||
          data(target).itemId ||
          'grid',
      );
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', TERMINALS_ACTIONS.retryFailedDrawerTab.selector, (_event, target) => {
      retryFailedDrawerTab(data(target).pendingTabId!);
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', TERMINALS_ACTIONS.closeFailedDrawerTab.selector, (_event, target) => {
      closeFailedDrawerTab(data(target).pendingTabId!);
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', TERMINALS_ACTIONS.createTerminalDrawerItem.selector, (event, target) => {
      const kind = data(target).itemId as 'default-shell' | 'ai-shell' | 'ai-chat';
      if (kind === 'default-shell') {
        void createProjectTerminal();
        return;
      }
      void aiLaunchConfiguration(kind, (event as MouseEvent).altKey, data(target).provider).then((configuration) => {
        if (!configuration) return;
        if (kind === 'ai-shell') void createProjectTerminal(configuration);
        else void createDrawerAIChat(configuration);
      });
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', TERMINALS_ACTIONS.openSavedConversation.selector, () => {
      void openSavedConversation();
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', TERMINALS_ACTIONS.createProjectTerminal.selector, () => {
      void createProjectTerminal();
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', TERMINALS_ACTIONS.closeProjectTab.selector, (event, target) => {
      event.stopPropagation();
      // A project tab and the phone strip's close action both carry the project id (HS2-7V2EEV).
      requestProjectClose([data(target.closest<HTMLElement>('[data-project-id]')!).projectId!]);
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', TERMINALS_ACTIONS.minimizeProjectTab.selector, (_event, target) => {
      const id = data(target).projectId;
      if (id) minimizeProjectTab(id);
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', TERMINALS_ACTIONS.restoreMinimizedProject.selector, (_event, target) => {
      const id = data(target).projectId;
      if (id) restoreMinimizedProject(id);
    }),
  );
  // Project Settings' Close Project… (HS2-C7GMQN) enters the same close flow as the tab button.
  lifetime.add(
    delegate(document.body, 'click', TERMINALS_ACTIONS.closeProjectFromSettings.selector, () => {
      const current = project();
      if (current) requestProjectClose([current.id]);
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', TERMINALS_ACTIONS.selectProjectCloseResource.selector, (_event, target) => {
      const state = projectCloseDialog.value,
        key = data(target).itemId;
      if (!state || !key || key === state.selectedKey) return;
      projectCloseDialog.value = { ...state, selectedKey: key, error: '' };
      restoreBorrowedProjectCloseTerminal(state);
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', TERMINALS_ACTIONS.cancelProjectClose.selector, () => {
      cancelProjectClose();
    }),
  );
  lifetime.add(
    delegateCapture(document.body, 'kui-request-close', TERMINALS_TARGETS.projectCloseDialog.selector, () => {
      if (!projectCloseDialog.value?.operation) cancelProjectClose();
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', TERMINALS_ACTIONS.confirmCloseProject.selector, () => {
      confirmProjectClose();
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', TERMINALS_ACTIONS.closeAllProjectResources.selector, () => {
      void closeAllProjectResources();
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', TERMINALS_ACTIONS.closeTerminalTab.selector, (event, target) => {
      event.stopPropagation();
      void closeTerminalIds([data(target.closest<HTMLElement>('[data-tab-kind="terminal"]')!).terminalId!]);
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', TERMINALS_ACTIONS.closeAiChatTab.selector, (event, target) => {
      event.stopPropagation();
      closeDrawerAIChat(data(target.closest<HTMLElement>('[data-tab-kind="ai-chat"]')!).chatId!);
    }),
  );
  lifetime.add(
    delegate(document.body, 'contextmenu', '[data-tab-kind]', (event, target) => {
      event.preventDefault();
      const pointer = event as MouseEvent,
        kind = data(target).tabKind as AppTabKind,
        id =
          kind === 'project'
            ? data(target).projectId!
            : kind === 'ai-chat'
              ? data(target).chatId!
              : data(target).terminalId!;
      const terminal =
        kind === 'terminal'
          ? terminalGroups.value
              .find((group) => group.projectId === project()?.id)
              ?.sessions.find((session) => session.id === id)
          : undefined;
      // HS2-6MNMVB: an orchestrator-started terminal offers a person's takeover.
      const orchestrator = terminal?.orchestrator;
      // HS2-H3S7YQ: a terminal tab offers choosing its session for feedback notifications.
      const feedback = kind === 'terminal' && terminal ? feedbackPrimaryMenuItem(terminal) : undefined;
      appTabContextMenu.value = {
        kind,
        id,
        direction: pointer.altKey ? 'left' : 'right',
        ...(kind === 'project' ? { restoreFailure: data(target).restoreFailure === 'true' } : {}),
        ...(orchestrator ? { orchestrator: { owner: orchestrator.owner } } : {}),
        ...(feedback ? { feedback } : {}),
        ...viewportSafeContextMenuPosition(pointer.clientX, pointer.clientY, window.innerWidth, window.innerHeight, {
          width: 296,
          height:
            (orchestrator?.owner === 'orchestrator' ? 266 : orchestrator ? 234 : 202) +
            (feedback ? (feedback.description && !feedback.disabledReason ? 52 : 32) : 0),
        }),
      };
    }),
  );
  lifetime.add(
    delegate(
      document.body,
      'click',
      '[data-action="project-tab-context-action"], [data-action="terminal-tab-context-action"]',
      (_event, target) => {
        const menu = appTabContextMenu.value;
        if (!menu) return;
        const ordered =
            menu.kind === 'project'
              ? [
                  ...projects.value.map((item) => item.id).filter((id) => !minimizedProjectIds.value.includes(id)),
                  ...projectRestoreFailures.value.map((item) => projectRestoreTabId(item.root)),
                ]
              : currentDrawerTabIds(project()?.id ?? ''),
          action = data(target).tabAction;
        if (action === 'minimize' && menu.kind === 'project') {
          appTabContextMenu.value = undefined;
          minimizeProjectTab(menu.id);
          return;
        }
        if ((action === 'orchestrator-takeover' || action === 'orchestrator-pause') && menu.kind === 'terminal') {
          appTabContextMenu.value = undefined;
          void takeOverHostedSession(menu.id, action === 'orchestrator-pause');
          return;
        }
        if ((action === 'feedback-primary-choose' || action === 'feedback-primary-clear') && menu.kind === 'terminal') {
          // A disabled command explains itself in its tooltip and does nothing.
          if (target.getAttribute('aria-disabled') === 'true') return;
          appTabContextMenu.value = undefined;
          const projectId = project()?.id;
          if (projectId)
            void setFeedbackPrimary(
              `${projectId}:${menu.id}`,
              action === 'feedback-primary-clear' ? 'clear' : 'choose',
            );
          return;
        }
        if (action === 'orchestrator-hand-back' && menu.kind === 'terminal') {
          appTabContextMenu.value = undefined;
          void handBackHostedSession(menu.id);
          return;
        }
        if (action === 'rename' && menu.kind === 'terminal') {
          const group = terminalGroups.value.find((item) => item.projectId === project()?.id),
            session = group?.sessions.find((item) => item.id === menu.id);
          appTabContextMenu.value = undefined;
          if (session) {
            terminalRenameSession += 1;
            terminalRename.value = {
              projectId: session.projectId,
              terminalId: session.id,
              value: session.title ?? session.id,
              session: terminalRenameSession,
              defaultName: session.named ? session.defaultTitle : undefined,
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
    ),
  );
  lifetime.add(
    wireTerminalRenameDialog(document.body, {
      rename: (name) => {
        const rename = terminalRename.value;
        if (!rename) return;
        saveTerminalName(rename.projectId, rename.terminalId, name);
        terminalRename.value = undefined;
      },
      reset: () => {
        const rename = terminalRename.value;
        if (!rename?.defaultName) return;
        resetTerminalName(rename.projectId, rename.terminalId);
        terminalRename.value = undefined;
      },
      dismiss: () => {
        terminalRename.value = undefined;
      },
    }),
  );
  return () => {
    lifetime.dispose();
  };
}
