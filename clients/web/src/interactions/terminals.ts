import { delegate, delegateCapture, type Signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import { type AiToolDefaults } from '../api';
import { browserRandomId } from '../browser-id';
import { type ProjectCloseDialogState } from '../components/project-close-dialog';
import { type AppTabKind } from '../components/project-tab-context-menu';
import {
  type TerminalCopyState,
  type TerminalEditMenuState,
  type TerminalPasteState,
} from '../components/terminal-clipboard-dialogs';
import { type TerminalDashboardGroup, type TerminalDashboardSession } from '../components/terminal-dashboard';
import { type TerminalRenameTarget } from '../components/terminal-rename-dialog';
import { type TerminalVisibilityNamePrompt } from '../components/terminal-visibility-dialog';
import { revealContextPopupMenu, viewportSafeContextMenuPosition } from '../context-menu-position';
import { copyWithSelection } from '../copy-text';
import { type DrawerTabCloseAction, drawerTabCloseIds } from '../drawer-tab-order';
import { TERMINALS_ACTIONS, TERMINALS_TARGETS } from '../interaction-attrs/terminals';
import { type DrawerAIChat } from '../project-drive';
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
  writeClipboardText,
} from '../terminal-clipboard';
import { adjustTerminalFit, terminalGridBasis } from '../terminal-grid-layout';
import {
  NO_TERMINAL_MODIFIERS,
  TERMINAL_KEY_EVENT,
  type TerminalModifier,
  type TerminalModifiers,
  type TerminalSpecialKey,
  toggleTerminalModifier,
} from '../terminal-keys';
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
import { wireTopLayerOverlays } from '../top-layer-overlay';
import { data } from './dom';
import { type Control, type Project } from './types';

export function allowInterruptedDrawerPopupShow(menu: { open: boolean; popup?: { active: boolean } }) {
  if (menu.open && menu.popup?.active) menu.popup.active = false;
}

/** Live application bindings used by this handler group. */
export interface TerminalInteractionsDependencies {
  readonly terminalDrawerBounds: Signal<{ width: number; height: number }>;
  readonly terminalDashboardSize: Signal<{ width: number; height: number }>;
  readonly terminalDrawerFitHigh: Signal<number>;
  readonly terminalFitAcross: Signal<number>;
  readonly terminalFitHigh: Signal<number>;
  terminalPreviewClickTimer: number | undefined;
  readonly terminalSession: (key?: string) => TerminalDashboardSession | undefined;
  readonly clearTerminalHalt: (key: string) => Promise<void>;
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
  readonly enterMobileTerminalFocus: (terminalId: string) => void;
  readonly exitMobileTerminalFocus: () => void;
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
  ) => AiToolDefaults | undefined;
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
  readonly terminalRename: Signal<TerminalRenameTarget | undefined>;
  readonly closeDrawerTabIds: (ids: readonly string[]) => Promise<void>;
  readonly saveTerminalName: (projectId: string, terminalId: string, name: string) => void;
  /** Clear a terminal's saved name so its tab shows the derived default again (HS2-2Q7KTX). */
  readonly resetTerminalName: (projectId: string, terminalId: string) => void;
}

/** Register this group only when the application wiring owner invokes it. */
export function wireTerminalInteractions(dependencies: TerminalInteractionsDependencies) {
  const lifetime = createScope();
  let terminalRenameSession = 0;
  const {
    terminalDrawerBounds,
    terminalDashboardSize,
    terminalDrawerFitHigh,
    terminalFitAcross,
    terminalFitHigh,
    terminalSession,
    clearTerminalHalt,
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
    enterMobileTerminalFocus,
    exitMobileTerminalFocus,
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
    resetTerminalName,
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
          .terminalId;
      exitMobileTerminalFocus();
      if (current && drawer?.dataset.mode === 'dedicated' && terminalId) focusDrawerTab(current.id, terminalId);
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', TERMINALS_ACTIONS.zoomTerminalGrid.selector, (_event, target) => {
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
    delegate(document.body, 'contextmenu', TERMINALS_TARGETS.terminalTile.selector, (event, target) => {
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
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', TERMINALS_ACTIONS.openTerminalContextMenu.selector, (event, target) => {
      event.preventDefault();
      const box = target.getBoundingClientRect();
      terminalContextMenu.value = {
        key: data(target).itemId!,
        ...viewportSafeContextMenuPosition(box.right, box.bottom, window.innerWidth, window.innerHeight, {
          width: 224,
          height: 104,
        }),
      };
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', TERMINALS_ACTIONS.clearTerminalHalt.selector, (_event, target) => {
      const key = data(target).itemId;
      terminalContextMenu.value = undefined;
      if (key) void clearTerminalHalt(key);
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
    delegate(document.body, 'click', TERMINALS_ACTIONS.toggleTerminalModifier.selector, (_event, target) => {
      const modifier = data(target).modifier as TerminalModifier | undefined;
      if (modifier) terminalModifiers.value = toggleTerminalModifier(terminalModifiers.value, modifier);
      keepTerminalFocus(keyBarViewport(target));
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', TERMINALS_ACTIONS.toggleTerminalFunctionRow.selector, (_event, target) => {
      terminalFunctionRow.value = !terminalFunctionRow.value;
      // Each row starts at its leading edge, so a swapped row never opens mid-scroll.
      target.closest('[data-component="terminal-key-bar"]')?.scrollTo({ left: 0 });
      keepTerminalFocus(keyBarViewport(target));
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', TERMINALS_ACTIONS.sendTerminalKey.selector, (_event, target) => {
      const key = data(target).key as TerminalSpecialKey | undefined,
        viewport = keyBarViewport(target);
      if (key && viewport) viewport.dispatchEvent(new CustomEvent(TERMINAL_KEY_EVENT, { detail: { key } }));
      keepTerminalFocus(viewport);
    }),
  );
  // Phone terminal clipboard (HS2-FRB545). The sheet keeps the viewport it was opened from, so a paste
  // lands in that terminal even if the drawer selection changed underneath the dialog.
  let clipboardViewport: HTMLElement | undefined,
    editMenuViewport: HTMLElement | undefined,
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
  // The long-press edit menu (HS2-KKP8YJ) acts on the terminal it was opened over; toolbar, pill, and key-bar
  // buttons act on the terminal of the drawer or tile that holds them.
  const clipboardTarget = (target: Element) => {
    if (!target.closest(TERMINALS_TARGETS.terminalEditMenu.selector)) return keyBarViewport(target);
    const viewport = editMenuViewport;
    editMenuViewport = undefined;
    terminalEditMenu.value = undefined;
    return viewport?.isConnected ? viewport : undefined;
  };
  lifetime.add(
    delegate(document.body, TERMINAL_EDIT_MENU_EVENT, TERMINALS_TARGETS.terminalViewport.selector, (event, target) => {
      const viewport = target as HTMLElement,
        { x, y } = (event as CustomEvent<TerminalEditMenuDetail>).detail;
      const { selection } = (event as CustomEvent<TerminalEditMenuDetail>).detail;
      editMenuViewport = viewport;
      terminalEditMenu.value = {
        ...viewportSafeContextMenuPosition(x, y, window.innerWidth, window.innerHeight, {
          width: 192,
          height: selection === true ? 144 : 96,
        }),
        selection: selection === true,
      };
    }),
  );
  // Escape dismisses the open edit menu even while a focused terminal holds the keyboard: xterm's
  // helper textarea handles Escape and stops it, so the document-level dismiss never sees it. Capture
  // it first, and do not also send ESC to the shell (HS2-B06X7Y).
  lifetime.add(
    delegateCapture(document.body, 'keydown', TERMINALS_TARGETS.terminalViewport.selector, (event) => {
      const keyboard = event as KeyboardEvent;
      if (!terminalEditMenuDismissKey(keyboard, Boolean(terminalEditMenu.peek()))) return;
      keyboard.preventDefault();
      keyboard.stopPropagation();
      editMenuViewport = undefined;
      terminalEditMenu.value = undefined;
    }),
  );
  // Copy from the long-press edit menu takes the terminal's touch selection straight to the clipboard
  // (HS2-EYR96N). The page is not inert here, so the shared off-screen selection fallback works.
  lifetime.add(
    delegate(document.body, 'click', TERMINALS_ACTIONS.copyTerminalSelection.selector, (_event, target) => {
      const viewport = clipboardTarget(target),
        text = viewport ? readTerminalViewportSelection(viewport) : undefined;
      if (!text) return;
      void writeClipboardText(text, navigator.clipboard as Clipboard | undefined, () => copyWithSelection(text)).then(
        (copied) => {
          showToast(copied ? terminalCopyMessage(text, true) : 'Could not copy the selection.');
        },
      );
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', TERMINALS_ACTIONS.copyTerminalText.selector, (_event, target) => {
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
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', TERMINALS_ACTIONS.confirmTerminalCopy.selector, () => {
      const field = sheetField('terminal-copy-text');
      if (!field) return;
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
    }),
  );
  lifetime.add(delegate(document.body, 'click', TERMINALS_ACTIONS.closeTerminalCopy.selector, closeCopySheet));
  lifetime.add(delegate(document.body, 'wa-hide', TERMINALS_TARGETS.terminalCopyDialog.selector, closeCopySheet));
  lifetime.add(
    delegate(document.body, 'click', TERMINALS_ACTIONS.pasteTerminalText.selector, (_event, target) => {
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
    }),
  );
  lifetime.add(
    delegate(document.body, 'submit', TERMINALS_ACTIONS.submitTerminalPaste.selector, (event, target) => {
      event.preventDefault();
      const text = target.querySelector<HTMLTextAreaElement>('textarea[name="terminal-paste-text"]')?.value ?? '',
        viewport = clipboardViewport;
      closePasteSheet();
      if (!viewport?.isConnected) return;
      if (text) pasteIntoTerminalViewport(viewport, text);
      // The dialog hands focus back to its opener once hidden; return it to the terminal instead.
      refocusAfterPaste = viewport;
    }),
  );
  lifetime.add(
    delegate(document.body, 'wa-after-hide', TERMINALS_TARGETS.terminalPasteDialog.selector, (event, target) => {
      if (event.target !== target) return;
      const viewport = refocusAfterPaste;
      refocusAfterPaste = undefined;
      // Web Awesome refocuses the opener in a task queued before this event; run after it.
      if (viewport?.isConnected)
        window.setTimeout(() => {
          keepTerminalFocus(viewport);
        });
    }),
  );
  lifetime.add(delegate(document.body, 'click', TERMINALS_ACTIONS.cancelTerminalPaste.selector, closePasteSheet));
  lifetime.add(delegate(document.body, 'wa-hide', TERMINALS_TARGETS.terminalPasteDialog.selector, closePasteSheet));
  lifetime.add(
    delegate(document.body, 'click', TERMINALS_ACTIONS.cycleMobileTerminalColumns.selector, () => {
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
  wireTerminalVisibilityTypeFilter(document.body, (types) => {
    terminalVisibilityFilter.value = types;
  });
  lifetime.add(
    delegate(document.body, 'click', TERMINALS_ACTIONS.openTerminalVisibility.selector, (event, target) => {
      event.stopImmediatePropagation();
      terminalVisibilityContextMenu.value = undefined;
      terminalVisibilityFilter.value = TERMINAL_VISIBILITY_TYPES;
      terminalVisibilityDialogScope.value = terminalVisibilityScopeFor(target);
    }),
  );
  lifetime.add(
    delegate(document.body, 'wa-hide', '[data-terminal-visibility-dialog]', (event, target) => {
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
        id = data(target).itemId;
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
    delegate(document.body, 'wa-hide', '[data-terminal-visibility-name-dialog]', () => {
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
      if ((event.target as Element).closest('button, input, textarea, select, a, [data-tab-kind="terminal"]')) return;
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
      const tab = target.closest<HTMLElement>('[data-tab-kind]');
      selectDrawerItem(tab?.dataset.terminalId || tab?.dataset.chatId || data(target).itemId || 'grid');
    }),
  );
  void delegateCapture(document.body, 'wa-show', '[data-terminal-drawer-create]', (event, target) => {
    if (event.target !== target) return;
    const menu = target as HTMLElement & { open: boolean; popup?: { active: boolean } };
    // Web Awesome keeps popup.active during its hide animation. Its next
    // showMenu call otherwise returns early and misses the keyboard listener.
    allowInterruptedDrawerPopupShow(menu);
  });
  lifetime.add(
    delegate(document.body, 'click', TERMINALS_ACTIONS.createTerminalDrawerItem.selector, (event, target) => {
      const kind = data(target).itemId as 'default-shell' | 'ai-shell' | 'ai-chat';
      if (kind === 'default-shell') {
        void createProjectTerminal();
        return;
      }
      const configuration = aiLaunchConfiguration(kind, (event as MouseEvent).altKey, data(target).provider);
      if (!configuration) return;
      if (kind === 'ai-shell') void createProjectTerminal(configuration);
      else void createDrawerAIChat(configuration);
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
      requestProjectClose([data(target.closest<HTMLElement>('[data-tab-kind="project"]')!).projectId!]);
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
    delegateCapture(document.body, 'wa-hide', TERMINALS_TARGETS.projectCloseDialog.selector, () => {
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
            menu.kind === 'project' ? projects.value.map((item) => item.id) : currentDrawerTabIds(project()?.id ?? ''),
          action = data(target).tabAction;
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
    delegate(document.body, 'submit', TERMINALS_ACTIONS.renameTerminalForm.selector, (event, target) => {
      event.preventDefault();
      const rename = terminalRename.value,
        name = target.querySelector<Control>('[name="terminal-name"]')?.value ?? '';
      if (!rename || !name.trim()) return;
      saveTerminalName(rename.projectId, rename.terminalId, name);
      terminalRename.value = undefined;
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', TERMINALS_ACTIONS.resetTerminalRename.selector, () => {
      const rename = terminalRename.value;
      if (!rename?.defaultName) return;
      resetTerminalName(rename.projectId, rename.terminalId);
      terminalRename.value = undefined;
    }),
  );
  lifetime.add(
    delegate(document.body, 'click', TERMINALS_ACTIONS.cancelTerminalRename.selector, () => {
      terminalRename.value = undefined;
    }),
  );
  lifetime.add(
    delegate(document.body, 'wa-hide', '[data-terminal-rename-dialog]', () => {
      terminalRename.value = undefined;
    }),
  );
  return () => {
    lifetime.dispose();
  };
}
