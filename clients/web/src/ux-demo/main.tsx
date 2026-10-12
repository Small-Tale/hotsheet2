import '@kerfjs/ui/webawesome.css';
import '@awesome.me/webawesome/dist/components/button/button.js';
import '@awesome.me/webawesome/dist/components/checkbox/checkbox.js';
import '@awesome.me/webawesome/dist/components/dropdown/dropdown.js';
import '@awesome.me/webawesome/dist/components/input/input.js';
// Kerf's Catalog composes a Workbench of Panes, Toolbars, and Lists and owns only its stage CSS, so
// the demo loads each composed primitive's stylesheet (Kerf 5.0.0-beta.55).
import '@kerfjs/ui/workbench.css';
import '@kerfjs/ui/pane.css';
import '@kerfjs/ui/toolbar.css';
import '@kerfjs/ui/text.css';
import '@kerfjs/ui/row.css';
import '@kerfjs/ui/list.css';
import '@kerfjs/ui/list-item.css';
import '@kerfjs/ui/list-header.css';
import '@kerfjs/ui/list-inset-text.css';
import '@kerfjs/ui/catalog.css';
import '@kerfjs/ui/floating-toolbar.css';
import '@kerfjs/ui/tab-bar.css';
import '@kerfjs/ui/select/register';
import '@kerfjs/ui/popup-menu/register';
import '@kerfjs/ui/document.css';
import '../hot-sheet-tokens.css';
import './style.css';

import { openPopupMenuAt, type PopupMenuElement } from '@kerfjs/ui/popup-menu';
import { revealCatalogEntry, wireCatalog } from '@kerfjs/ui/wire-catalog';
import { wireNavStack } from '@kerfjs/ui/wire-nav-stack';
import { wireResizableRegions } from '@kerfjs/ui/wire-resizable-regions';
import { reorderTabs, wireTabBars } from '@kerfjs/ui/wire-tab-bars';
import { wireTokenSearchFields } from '@kerfjs/ui/wire-token-search-fields';
import { wireToolbarVisibility } from '@kerfjs/ui/wire-toolbar-visibility';
import { wireWorkbench } from '@kerfjs/ui/wire-workbench';
import { delegate, delegateCapture, mount } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import type { CommandDropTarget } from '../command-order';
import { attachmentGalleryKeyboardAction } from '../components/attachment-gallery';
import { COMMAND_EDITOR_DIALOG_ID } from '../components/command-settings-editor';
import { PROJECT_TAB_BAR_ID } from '../components/project-tab-bar';
import { showQuickTicketComposer } from '../components/quick-ticket-composer';
import type { SettingsCategory } from '../components/settings-navigation';
import { TAG_CHIP_REMOVE_ACTION } from '../components/tag-chip';
import { showTicketReaderDialog } from '../components/ticket-reader';
import { eventTargetsContextMenu } from '../components/ticket-row-context-menu';
import { statusMenuAnchorY } from '../components/ticket-status-menu';
import { addTicketTag, removeTicketTag } from '../components/ticket-tag-editor';
import { nextWorkspaceSort, wireWorkspaceOverflowKeyboard } from '../components/workspace-header';
import { type ContextPopupMenuElement, openContextPopupMenu, revealContextPopupMenu } from '../context-menu-position';
import { viewportSafeContextMenuPosition } from '../context-menu-position';
import { withControlledOpen } from '../controlled-open';
import { createDebouncedAutosave } from '../debounced-autosave';
import { parseFeedbackChoices, updateFeedbackChoiceSelection } from '../feedback-choices';
import { restoreInlineSearchCaret } from '../inline-search-caret';
import { COMMANDS_AND_AI_ACTIONS } from '../interaction-attrs/commands-and-ai';
import { INSPECTOR_AND_EDITOR_ACTIONS } from '../interaction-attrs/inspector-and-editor';
import { NOTIFICATIONS_AND_LINKS_ACTIONS } from '../interaction-attrs/notifications-and-links';
import { TERMINALS_ACTIONS } from '../interaction-attrs/terminals';
import { TICKET_SELECTION_ACTIONS } from '../interaction-attrs/ticket-selection';
import { wireTicketSearchFields } from '../interactions/ticket-search-field';
import { clickBeginsMarkdownEdit, keyBeginsMarkdownEdit, repeatPressWouldLeaveNewEditor } from '../markdown-click-edit';
import { nextMobileTerminalColumns } from '../mobile-terminal-columns';
import type { PermissionAutomationAction } from '../permission-notifications';
import { keepSearchOpenForShadowTarget } from '../search-collapse';
import { terminalCopyMessage, terminalCopySelection } from '../terminal-clipboard';
import {
  consumeTerminalModifiers,
  encodeTerminalKey,
  type TerminalModifier,
  type TerminalSpecialKey,
  toggleTerminalModifier,
} from '../terminal-keys';
import { wireTerminalVisibilityTypeFilter } from '../terminal-visibility-filter';
import { normalizeTicketTitleField, ticketTitleKeyFinishesEdit } from '../ticket-title-editing';
import { wireTopLayerOverlays } from '../top-layer-overlay';
import { wireTicketRailToolbarVisibility, wireWorkspaceToolbarVisibility } from '../workspace-toolbar-visibility';
import {
  aiConversationDemoOpen,
  aiConversationDraft,
  aiConversationPresentation,
  aiConversationProvider,
  aiConversationProviderLabel,
  aiConversationSaveCount,
  aiConversationScenario,
  resetAIConversationDemo,
} from './ai-conversation-demo';
import { appLoadingDemoProgress } from './app-loading-indicator-demo';
import {
  addDemoProject,
  appShellSettings,
  closeAllProjectTabs,
  closeOtherProjectTabs,
  closeProjectTab,
  closeProjectTabsToRight,
  projectTabs,
  resetAppShellDemo,
  resizeDemoCollapsed,
  selectProjectTab,
  setRegionSize,
  shellEvent,
  shellInspectorSize,
  shellMode,
  shellSidebarSize,
  shellSidebarVisible,
  shellStatsProjectName,
  shellTerminalDrawerVisible,
} from './app-shell-demo';
import {
  type BulkTicketDialogScenario,
  closeBulkTicketDialogDemo,
  openBulkTicketDialogDemo,
  resetBulkTicketDialogDemo,
  setBulkTicketDialogScenario,
} from './bulk-ticket-dialog-demo';
import {
  type CommandRunDialogPresentation,
  confirmStopCommandRunDemo,
  dismissCommandRunDialogDemo,
  resetCommandRunDialogDemo,
  setCommandRunDialogPresentation,
  showCommandRunDialogDemo,
} from './command-run-dialog-demo';
import { DemoApp } from './components/demo-app';
import type { FormControl, TicketSourceScenario } from './components/demo-content';
import {
  appErrorDemoVisible,
  CLIPBOARD_DEMO_TEXT,
  clipboardDemoCopy,
  clipboardDemoGeneration,
  clipboardDemoOutput,
  clipboardDemoPaste,
  drawerFocusDemoColumns,
  editMenuDemo,
  editMenuDemoOutput,
  keyBarDemoFunctionRow,
  keyBarDemoModifiers,
  keyBarDemoOutput,
  projectCloseDemoSelection,
  renameDemoOutput,
  renameDemoSession,
  renameDemoTarget,
  renameDemoTargets,
  terminalDashboardContextMenu,
  terminalDashboardHaltCleared,
  ticketSourceScenario,
} from './components/demo-content';
import {
  applyDisplayPreferences,
  backgroundStyle,
  collapsed,
  contextMenu,
  increasedContrast,
  onDemoSelected,
  reducedMotion,
  saveDisplayPreferences,
  selectDemo,
  selectedId,
  settingsOpen,
  tabContextMenu,
  theme,
  URL_PARAM,
} from './components/demo-state';
import { confidenceBadgeSettings, resetConfidenceBadgeDemo } from './confidence-badge-demo';
import { confidenceCalibrationSettings, resetConfidenceCalibrationDemo } from './confidence-calibration-demo';
import { connectionDetailsScenario, resetConnectionDetailsDemo } from './connection-details-demo';
import {
  editingNoteId,
  inspectorBlockedReason,
  inspectorBlockedReasonDraft,
  inspectorBlockedReasonEditing,
  markdownAppearance,
  type MarkdownEditorAppearanceDemo,
  type MarkdownEditorInsetDemo,
  markdownEvent,
  markdownExpanded,
  markdownInset,
  markdownMode,
  markdownSavedValue,
  markdownValue,
  noteComposerValue,
  noteDemoNotes,
  noteDraft,
  readerAttachments,
  readerDialogOpen,
  readerFeedbackChoiceSelections,
  readerLargeText,
  readerNotes,
  readerTab,
  readerTitle,
  readerTitleDraft,
  readerTitleEditing,
  resetMarkdownEditorDemo,
} from './content-components-demo';
import {
  resetContentTransitionDemo,
  transitionDirection,
  transitionSide,
  transitionStyle,
} from './content-transition-demo';
import { DEMO_ACTIONS, DEMO_COMPONENTS, DEMO_FIELDS, DEMO_MARKERS } from './demo-actions';
import { closeHs1MigrationDialogDemo, openHs1MigrationDialogDemo } from './dialog-layout-demo';
import { haltedSessionDemoResult, haltedSessionDemoVisible } from './halted-session-demo';
import {
  notWorkingDemoEvent,
  notWorkingDemoFiles,
  notWorkingDemoNote,
  notWorkingDemoOpen,
} from './not-working-dialog-demo';
import {
  permissionRequestSettings,
  resetPermissionRequestDemo,
  resetPermissionRequestDemoCountdown,
  startPermissionRequestDemoCountdown,
  stopPermissionRequestDemoAutomation,
} from './permission-components-demo';
import { wireProjectDialogDemo } from './project-dialog-demo';
import {
  addCommandEditorGroup,
  addCommandEditorSetting,
  clampProjectSidebarHeight,
  closeCommandEditorDemo,
  collapsedCommandGroups,
  commandEditorCommands,
  commandEditorEditingId,
  commandEditorIconSearch,
  commandEditorSelection,
  commandGroupExpanded,
  deleteCommandEditorGroup,
  deleteCommandEditorSetting,
  driveRunning,
  openCommandEditorDemo,
  projectSidebarHeight,
  reorderCommandEditorSettings,
  runningCommandId,
  selectCommandEditorRow,
  selectedViewId,
  sidebarCommands,
  sidebarEvent,
  sidebarViews,
  updateCommandEditorField,
} from './project-sidebar-demo';
import {
  changeEvidenceDemoView,
  repositoryDemoComparison,
  repositoryDemoDetailActive,
  repositoryDemoEvent,
  repositoryDemoExpandedCommits,
  repositoryDemoFileMenu,
  repositoryDemoScenario,
  repositoryDemoView,
  resetRepositoryStatusDemo,
} from './repository-status-demo';
import { resetSettingsWorkspaceDemo, settingsWorkspaceSettings } from './settings-workspace-demo';
import { resetStatusBadgeDemo, statusBadgeSettings } from './status-badge-demo';
import { resetTagChipDemo, tagChipSettings } from './tag-chip-demo';
import { syncTerminalDemoViewports } from './terminal-demo';
import {
  cancelTerminalVisibilityDemoName,
  closeTerminalVisibilityDemo,
  closeTerminalVisibilityDemoContextMenu,
  promptAddTerminalVisibilityDemoGroup,
  promptRenameTerminalVisibilityDemoGroup,
  removeTerminalVisibilityDemoGroup,
  selectTerminalVisibilityDemoGroup,
  setAllTerminalVisibilityDemo,
  showTerminalVisibilityDemo,
  showTerminalVisibilityDemoContextMenu,
  submitTerminalVisibilityDemoName,
  terminalVisibilityDemoTypes,
  toggleTerminalVisibilityDemo,
} from './terminal-visibility-demo';
import {
  collectionTickets,
  recordCollectionEvent,
  selectAllCollectionTickets,
  selectCollectionTicket,
  toggleCollectionTicketUpNext,
} from './ticket-collections-demo';
import {
  attachmentDemoLabelEditor,
  attachmentDemoMenu,
  closeAttachmentDemoMenu,
  galleryDemoDrawMode,
  galleryDemoMarkup,
  galleryDemoMuted,
  galleryDemoPlayhead,
  galleryDemoPlaying,
  galleryDemoSelectedAnnotation,
  galleryDemoVideoAnnotations,
  galleryDemoVolume,
  galleryDemoVolumeOpen,
  regroupAttachmentDemo,
  setGalleryDemo,
  setGalleryDemoAnnotationEndpoint,
  shiftGalleryDemo,
  showAttachmentDemoMenu,
  statusMenuDemoStartedPhase,
  statusMenuDemoStatus,
  zoomGalleryDemo,
} from './ticket-metadata-demo';
import { resetTicketRowDemo, ticketRowSettings } from './ticket-row-demo';
import {
  resetTicketSearchDemo,
  ticketSearchDemoCollapsibleOpen,
  ticketSearchDemoGrowOpen,
  ticketSearchDemoModel,
  ticketSearchDemoModels,
  ticketSearchDemoRowOpen,
  toggleDemoHelp,
} from './ticket-search-field-demo';
import { toolbarGroupDemoMode } from './toolbar-control-group-demo';
import {
  composerAttachments,
  composerCategory,
  composerDetails,
  composerExpanded,
  composerMultipleSources,
  composerSourcePick,
  composerTitle,
  composerUpNext,
  createDemoTicket,
  focusWorkspaceSearch,
  ignoreWorkspaceDemoPermission,
  inspectorCategory,
  inspectorLiveClaim,
  type InspectorLiveClaimDemo,
  inspectorOpen,
  inspectorPriority,
  inspectorStartedPhase,
  inspectorStatus,
  inspectorTab,
  inspectorTags,
  inspectorTitle,
  inspectorTitleDraft,
  inspectorTitleEditing,
  resetQuickTicketComposerDemo,
  resetTicketInspectorDemo,
  resetWorkspaceDemoNotifications,
  resolveWorkspaceDemoPermission,
  terminalRailDemoTicket,
  toggleWorkspaceDemoUpNext,
  workspaceMode,
  workspaceSearchHelpOpen,
  workspaceSearchModel,
  workspaceSearchOpen,
  workspaceSort,
  workspaceSortDirection,
} from './workspace-components-demo';

const markdownAutosave = createDebouncedAutosave((value: string) => {
  markdownSavedValue.value = value;
  markdownEvent.value = 'Markdown autosaved.';
  return Promise.resolve(true);
});
const blockedReasonAutosave = createDebouncedAutosave((value: string) => {
  inspectorBlockedReason.value = value.trim();
  return Promise.resolve(true);
});
const noteAutosave = createDebouncedAutosave(({ id, value }: { id: string; value: string }) => {
  readerNotes.value = readerNotes.value.map((note) => (note.id === id ? { ...note, body: value } : note));
  noteDemoNotes.value = noteDemoNotes.value.map((note) => (note.id === id ? { ...note, body: value } : note));
  return Promise.resolve(true);
});
const titleAutosave = createDebouncedAutosave((value: string) => {
  inspectorTitle.value = value.trim();
  return Promise.resolve(true);
});
const readerTitleAutosave = createDebouncedAutosave((value: string) => {
  readerTitle.value = value.trim();
  return Promise.resolve(true);
});
const tagsAutosave = createDebouncedAutosave((value: string[]) => {
  inspectorTags.value = value;
  return Promise.resolve(true);
});
let sidebarResizeDrag: { startY: number; startHeight: number } | undefined;
const usesCollectionState = () =>
  [
    'ticket-list',
    'ticket-board',
    'workspace-header',
    'terminal-ticket-rail',
    'quick-ticket-composer',
    'app-shell',
  ].includes(selectedId.value);

const root = document.querySelector<HTMLElement>('#ux-demo')!;
// Every delegated demo listener on `root` registers its disposer here, so the catalog's wiring has one
// teardown (`demoListeners.dispose()`) instead of hundreds of discarded disposers (HS2-G838PZ).
const demoListeners = createScope();
applyDisplayPreferences();
mount(root, withControlledOpen(root, DemoApp));
demoListeners.add(wireToolbarVisibility(root));
demoListeners.add(wireWorkspaceToolbarVisibility(root));
demoListeners.add(wireTicketRailToolbarVisibility(root));
demoListeners.add(wireProjectDialogDemo(root));
// Demo stages render context-mode PopupMenus statically; keep every one open so the catalog shows
// the menu itself (the app opens them from its own signals, HS2-2EHD8R).
const openStagedContextMenus = () => {
  for (const menu of root.querySelectorAll<ContextPopupMenuElement>('[data-context-menu]'))
    if (!menu.open) openContextPopupMenu(menu);
};
new MutationObserver(openStagedContextMenus).observe(root, { childList: true, subtree: true });
// Magnified terminals are manual popovers in the top layer, as in the app (HS2-Z9PQSC).
wireTopLayerOverlays(root);
openStagedContextMenus();
// The app clears its menu signals through capture-phase Escape and outside-press listeners; the
// demo's bubble-phase handlers run after Web Awesome has consumed the key, so mirror Web Awesome's
// own close (`wa-hide` on the menu root itself, not on a closing submenu) into the demo state.
demoListeners.add(
  delegate(root, 'wa-hide', '[data-context-menu]', (event, target) => {
    if (event.target !== target) return;
    const surface = (target as HTMLElement).dataset.contextMenu;
    // A selection hides the menu before its item click reaches the delegated handlers (microtasks flush
    // between listeners), so clear the state in a later task, after that click has finished dispatching.
    window.setTimeout(() => {
      if (surface === 'ticket') contextMenu.value = undefined;
      else if (surface === 'app-tab') tabContextMenu.value = undefined;
      else if (surface === 'terminal') terminalDashboardContextMenu.value = undefined;
      else if (surface === 'terminal-edit') editMenuDemo.value = undefined;
      else if (surface === 'attachment') closeAttachmentDemoMenu();
      else if (surface === 'terminal-visibility-group') closeTerminalVisibilityDemoContextMenu();
    }, 0);
  }),
);
// The AppShell demo's rails resize through Kerf's Workbench wiring, as in the app (HS2-P289N2).
wireWorkbench(root, {
  id: 'app',
  panels: { leftRail: { size: shellSidebarSize }, rightRail: { size: shellInspectorSize } },
});
// Tab strips reorder, roam, and edge-autoscroll through the same Kerf wiring the application installs on
// its body (HS2-KB5PJQ); a reorder only rewrites the controlled demo state that owns that strip.
demoListeners.add(
  wireTabBars(root, {
    activation: 'manual',
    onReorder: ({ barId, sourceId, targetId, position, source }) => {
      if (barId === PROJECT_TAB_BAR_ID)
        projectTabs.value = reorderTabs(projectTabs.value, (tab) => tab.id, sourceId, targetId, position);
      shellEvent.value = `Moved ${sourceId} ${position} ${targetId} by ${source}.`;
    },
  }),
);
// ResizableRegion demos resize through Kerf's pointer and separator-keyboard wiring; the commit only
// mirrors the settled size into the demo signal that renders it (HS2-KB5PJQ).
demoListeners.add(
  wireResizableRegions(root, {
    onCommit: ({ id, size }) => {
      setRegionSize(id, size);
    },
  }),
);
// The same shared TicketSearchField wiring the application uses, routed to demo state (HS2-N5G6JS);
// it is wired before Kerf's so its focus handlers see a chip before Kerf removes or expands it.
const demoSearchModel = (id: string) => (id === 'workspace-search' ? workspaceSearchModel : ticketSearchDemoModel(id));
demoListeners.add(
  wireTicketSearchFields(root, {
    applyDate: (id, _prefix, value) => {
      demoSearchModel(id)?.commit(value);
    },
    toggleHelp: (id) => {
      if (id === 'workspace-search') workspaceSearchHelpOpen.value = !workspaceSearchHelpOpen.value;
      else toggleDemoHelp(id);
    },
    clear: (id) => {
      if (id === 'workspace-search') {
        workspaceSearchHelpOpen.value = false;
        queueMicrotask(() => {
          focusWorkspaceSearch(root);
        });
      }
    },
    removeToken: () => undefined,
    editToken: () => undefined,
  }),
);
// Kerf owns editor chrome, collapsible behavior, and, through the registered models, parsing,
// chips, suggestions, chip edit/removal, and clear (HS2-5JXBQY).
demoListeners.add(
  wireTokenSearchFields(root, {
    models: { 'workspace-search': workspaceSearchModel, ...ticketSearchDemoModels },
    // Enter commits a trailing filter through the model; the rebuilt editor gets its caret back at the end.
    onSubmit: ({ id }) => {
      restoreInlineSearchCaret(root, `[data-token-search-editor="${id}"]`);
    },
    collapsible: {
      keepOpenOn: keepSearchOpenForShadowTarget,
      signals: {
        'workspace-search': workspaceSearchOpen,
        'ticket-search-demo-collapsible': ticketSearchDemoCollapsibleOpen,
        'ticket-search-demo-grow': ticketSearchDemoGrowOpen,
        'ticket-search-demo-row': ticketSearchDemoRowOpen,
      },
    },
  }),
);
// Kerf owns the catalog chrome: selection with `?component=` mirrored through replaceState, the sidebar
// (a transient overlay on a small screen through the app-owned `collapsed` flag), and the theme,
// contrast, reduced-motion, and preview-background controls (HS2-HMVBZW).
demoListeners.add(
  wireCatalog(root, {
    collapsed,
    onSelect: selectDemo,
    onToggleSidebar: () => {
      collapsed.value = !collapsed.value;
      saveDisplayPreferences();
    },
    onToggleTheme: () => {
      theme.value = theme.value === 'dark' ? 'light' : 'dark';
      applyDisplayPreferences();
      saveDisplayPreferences();
    },
    onToggleContrast: () => {
      increasedContrast.value = !increasedContrast.value;
      applyDisplayPreferences();
      saveDisplayPreferences();
    },
    onToggleMotion: () => {
      reducedMotion.value = !reducedMotion.value;
      applyDisplayPreferences();
      saveDisplayPreferences();
    },
    onSelectBackground: (style) => {
      backgroundStyle.value = style;
      saveDisplayPreferences();
    },
    urlParam: URL_PARAM,
    revealSelection: true,
  }),
);
demoListeners.add(
  onDemoSelected((id) => {
    terminalDashboardContextMenu.value = undefined;
    if (id === 'command-run-dialog') showCommandRunDialogDemo(root);
    if (id !== 'ticket-search-field') resetTicketSearchDemo();
  }),
);
revealCatalogEntry(root, selectedId.value, { block: 'center' });
if (selectedId.value === 'ticket-reader') queueMicrotask(() => showTicketReaderDialog(root, 'ux-demo-ticket-reader'));
if (selectedId.value === 'command-run-dialog') showCommandRunDialogDemo(root);
const terminalDemoMounts = new Map<HTMLElement, () => void>();
const syncDemoTerminals = () => {
  syncTerminalDemoViewports(root, terminalDemoMounts);
};
new MutationObserver(syncDemoTerminals).observe(root, { childList: true, subtree: true });
// The TerminalTicketRail demo's NavStack animates push/pop and reports Back through `wireNavStack`
// (HS2-FY06N4), rewired whenever the demo mounts a new stack.
let demoRailNavStack: { section: Element; dispose: () => void } | undefined;
function syncDemoRailNavStack() {
  const section = root.querySelector('.terminal-ticket-rail-demo [data-component="nav-stack"]');
  if (section === (demoRailNavStack?.section ?? null)) return;
  demoRailNavStack?.dispose();
  demoRailNavStack = section
    ? {
        section,
        dispose: wireNavStack(section, {
          onBack: () => {
            terminalRailDemoTicket.value = undefined;
            recordCollectionEvent('Back to the ticket list');
          },
        }),
      }
    : undefined;
}
new MutationObserver(syncDemoRailNavStack).observe(root, { childList: true, subtree: true });
queueMicrotask(syncDemoRailNavStack);
// The compact Repository Status drill-down pops back to its list the same way (HS2-3B8345).
let demoRepositoryNavStack: { section: Element; dispose: () => void } | undefined;
function syncDemoRepositoryNavStack() {
  const section = root.querySelector('.repository-status-demo [data-component="nav-stack"]');
  if (section === (demoRepositoryNavStack?.section ?? null)) return;
  demoRepositoryNavStack?.dispose();
  demoRepositoryNavStack = section
    ? {
        section,
        dispose: wireNavStack(section, {
          onBack: () => {
            repositoryDemoDetailActive.value = false;
            repositoryDemoEvent.value = 'Back to repository views.';
          },
        }),
      }
    : undefined;
}
new MutationObserver(syncDemoRepositoryNavStack).observe(root, { childList: true, subtree: true });
queueMicrotask(syncDemoRepositoryNavStack);
queueMicrotask(syncDemoTerminals);
startPermissionRequestDemoCountdown(root, () => selectedId.value === 'permission-request');
void delegate(root, 'click', '[data-reset-halt-demo]', () => {
  haltedSessionDemoVisible.value = true;
});
for (const [action, result] of [
  [
    NOTIFICATIONS_AND_LINKS_ACTIONS.openHaltedSession,
    'Open session returns to the originating project and session without clearing its halt indicator.',
  ],
  [NOTIFICATIONS_AND_LINKS_ACTIONS.dismissHaltedSession, 'Prompt dismissed. The session remains halted.'],
  [
    NOTIFICATIONS_AND_LINKS_ACTIONS.pauseNotifications,
    'Notifications paused. Unseen halted sessions wait until Resume.',
  ],
] as const)
  void delegate(root, 'click', action.selector, () => {
    if (selectedId.value !== 'halted-session-popup') return;
    haltedSessionDemoResult.value = result;
    haltedSessionDemoVisible.value = false;
  });
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.toggleSettings.selector, () => {
    settingsOpen.value = !settingsOpen.value;
  }),
);
demoListeners.add(
  delegate(root, 'click', '[data-app-error-demo-show]', () => {
    appErrorDemoVisible.value = true;
  }),
);
demoListeners.add(
  delegate(root, 'click', '[data-component="app-error"] [data-action="dismiss-app-error"]', () => {
    appErrorDemoVisible.value = false;
  }),
);
function commandEditorRowId(target: Element): string | undefined {
  return target.closest<HTMLElement>('[data-command-id]')?.dataset.commandId;
}
let draggedCommandEditorIds: string[] = [];
function clearCommandEditorDropIndicators() {
  root
    .querySelectorAll<HTMLElement>('[data-command-drop-position]')
    .forEach((element) => delete element.dataset.commandDropPosition);
  root
    .querySelectorAll<HTMLElement>('[data-command-drop-active]')
    .forEach((element) => delete element.dataset.commandDropActive);
}
function clearCommandEditorDrag() {
  draggedCommandEditorIds = [];
  root
    .querySelectorAll<HTMLElement>('[data-command-dragging]')
    .forEach((element) => delete element.dataset.commandDragging);
  clearCommandEditorDropIndicators();
}
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.editCommandSetting.selector, (_event, target) => {
    const id = commandEditorRowId(target);
    if (id) openCommandEditorDemo(id);
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.addCommandSetting.selector, () => {
    addCommandEditorSetting();
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.closeCommandEditor.selector, () => {
    closeCommandEditorDemo();
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.deleteCommandSetting.selector, (_event, target) => {
    const id = commandEditorRowId(target);
    if (id) deleteCommandEditorSetting(id);
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.addCommandGroup.selector, () => {
    addCommandEditorGroup();
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.deleteCommandGroup.selector, (_event, target) => {
    const group = target.closest<HTMLElement>('[data-group]')?.dataset.group;
    if (group) deleteCommandEditorGroup(group);
  }),
);
demoListeners.add(
  delegate(root, 'dblclick', '.command-settings-editor__row', (event, target) => {
    if ((event.target as Element).closest('.command-settings-editor__row-menu')) return;
    const id = (target as HTMLElement).dataset.commandId;
    if (id) openCommandEditorDemo(id);
  }),
);
demoListeners.add(
  delegate(root, 'contextmenu', '.command-settings-editor__row', (event, target) => {
    const menu = target.querySelector<HTMLElement & { show?(): void }>('.command-settings-editor__row-menu');
    if (!menu) return;
    event.preventDefault();
    menu.show?.();
  }),
);
demoListeners.add(
  delegate(root, 'click', '.command-settings-editor__row', (event, target) => {
    if ((event.target as Element).closest('.command-settings-editor__row-menu, .command-settings-editor__row-grip'))
      return;
    const id = (target as HTMLElement).dataset.commandId;
    if (!id) return;
    const mouse = event as MouseEvent;
    selectCommandEditorRow(id, { toggle: mouse.metaKey || mouse.ctrlKey, range: mouse.shiftKey });
  }),
);
demoListeners.add(
  delegate(root, 'dragstart', '.command-settings-editor__row', (event, target) => {
    const element = target as HTMLElement,
      id = element.dataset.commandId;
    if (!id) return;
    const selection = commandEditorSelection.value;
    draggedCommandEditorIds =
      selection.length > 1 && selection.includes(id)
        ? commandEditorCommands.value.map((command) => command.id).filter((commandId) => selection.includes(commandId))
        : [id];
    if (draggedCommandEditorIds.length <= 1) selectCommandEditorRow(id, {});
    root.querySelectorAll<HTMLElement>('.command-settings-editor__row').forEach((row) => {
      if (draggedCommandEditorIds.includes(row.dataset.commandId ?? '')) row.dataset.commandDragging = 'true';
    });
    const transfer = (event as DragEvent).dataTransfer;
    if (transfer) {
      transfer.effectAllowed = 'move';
      transfer.setData('text/plain', draggedCommandEditorIds.join(','));
    }
  }),
);
demoListeners.add(
  delegate(root, 'dragover', '.command-settings-editor__list', (event) => {
    if (!draggedCommandEditorIds.length) return;
    const drag = event as DragEvent,
      over = drag.target as Element,
      row = over.closest<HTMLElement>('[data-command-id]');
    clearCommandEditorDropIndicators();
    if (row && row.dataset.commandId && !draggedCommandEditorIds.includes(row.dataset.commandId)) {
      drag.preventDefault();
      const bounds = row.getBoundingClientRect();
      row.dataset.commandDropPosition = drag.clientY < bounds.top + bounds.height / 2 ? 'before' : 'after';
      return;
    }
    const container = over.closest<HTMLElement>('[data-command-group-drop]');
    if (container) {
      drag.preventDefault();
      container.dataset.commandDropActive = 'true';
    }
  }),
);
demoListeners.add(
  delegate(root, 'drop', '.command-settings-editor__list', (event) => {
    const sources = draggedCommandEditorIds;
    if (!sources.length) {
      clearCommandEditorDrag();
      return;
    }
    const drag = event as DragEvent,
      over = drag.target as Element,
      row = over.closest<HTMLElement>('[data-command-id]');
    drag.preventDefault();
    let dropTarget: CommandDropTarget | undefined;
    if (row && row.dataset.commandId && !sources.includes(row.dataset.commandId)) {
      const bounds = row.getBoundingClientRect();
      dropTarget = {
        kind: 'row',
        id: row.dataset.commandId,
        position: drag.clientY < bounds.top + bounds.height / 2 ? 'before' : 'after',
      };
    } else {
      const container = over.closest<HTMLElement>('[data-command-group-drop]');
      if (container) dropTarget = { kind: 'group', group: container.dataset.commandGroupDrop ?? '' };
    }
    clearCommandEditorDrag();
    if (dropTarget) reorderCommandEditorSettings(sources, dropTarget);
  }),
);
demoListeners.add(delegate(root, 'dragend', '.command-settings-editor__row', clearCommandEditorDrag));
demoListeners.add(
  delegate(root, 'input', '[data-command-field]', (_event, target) => {
    const input = target as HTMLInputElement;
    const id = commandEditorRowId(target);
    if (id && input.name) updateCommandEditorField(id, input.name, input.value);
  }),
);
demoListeners.add(
  delegate(root, 'input', DEMO_FIELDS.commandIconSearch.selector, (_event, target) => {
    commandEditorIconSearch.value = (target as HTMLInputElement).value;
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.selectCommandIcon.selector, (_event, target) => {
    const id = commandEditorRowId(target);
    const name = (target as HTMLElement).dataset.iconName;
    if (id && name) updateCommandEditorField(id, 'icon', name);
  }),
);
demoListeners.add(
  delegateCapture(
    root,
    'toggle',
    `#${COMMAND_EDITOR_DIALOG_ID}`,
    (event) => {
      if ((event as ToggleEvent).newState !== 'closed') return;
      commandEditorEditingId.value = undefined;
      commandEditorIconSearch.value = '';
    },
    { match: 'direct' },
  ),
);
demoListeners.add(delegate(root, 'click', DEMO_ACTIONS.openHs1MigrationDemo.selector, openHs1MigrationDialogDemo));
demoListeners.add(delegate(root, 'click', DEMO_ACTIONS.dismissHs1Migration.selector, closeHs1MigrationDialogDemo));
demoListeners.add(delegate(root, 'wa-hide', DEMO_COMPONENTS.hs1MigrationDialog.selector, closeHs1MigrationDialogDemo));
function showTerminalDashboardContextMenu(target: HTMLElement, x: number, y: number): void {
  terminalDashboardContextMenu.value = {
    key: target.dataset.terminalKey ?? target.dataset.itemId ?? '',
    ...viewportSafeContextMenuPosition(x, y, innerWidth, innerHeight, { width: 224, height: 104 }),
  };
}
demoListeners.add(
  delegate(root, 'contextmenu', DEMO_COMPONENTS.terminalTile.selector, (event, target) => {
    event.preventDefault();
    const pointer = event as MouseEvent;
    showTerminalDashboardContextMenu(target as HTMLElement, pointer.clientX, pointer.clientY);
  }),
);
demoListeners.add(
  delegate(root, 'click', TERMINALS_ACTIONS.clearTerminalHalt.selector, () => {
    terminalDashboardHaltCleared.value = true;
    terminalDashboardContextMenu.value = undefined;
  }),
);
// The TerminalKeyBar demo exercises the same modifier and Fn transitions as production and shows the
// exact bytes each key would send (HS2-CKS78M).
demoListeners.add(
  delegate(root, 'click', '.terminal-key-bar-demo [data-action="toggle-terminal-modifier"]', (_event, target) => {
    const modifier = (target as HTMLElement).dataset.modifier as TerminalModifier | undefined;
    if (modifier) keyBarDemoModifiers.value = toggleTerminalModifier(keyBarDemoModifiers.value, modifier);
  }),
);
demoListeners.add(
  delegate(root, 'click', '.terminal-key-bar-demo [data-action="toggle-terminal-function-row"]', (_event, target) => {
    keyBarDemoFunctionRow.value = !keyBarDemoFunctionRow.value;
    target.closest('[data-component="terminal-key-bar"]')?.scrollTo({ left: 0 });
  }),
);
demoListeners.add(
  delegate(root, 'click', '.terminal-key-bar-demo [data-action="send-terminal-key"]', (_event, target) => {
    const key = (target as HTMLElement).dataset.key as TerminalSpecialKey,
      bytes = encodeTerminalKey(key, keyBarDemoModifiers.value);
    keyBarDemoModifiers.value = consumeTerminalModifiers(keyBarDemoModifiers.value);
    keyBarDemoOutput.value = `${key} → ${JSON.stringify(bytes).slice(1, -1)}`;
  }),
);
demoListeners.add(
  delegate(
    root,
    'click',
    '.terminal-key-bar-demo [data-action="copy-terminal-text"], .terminal-key-bar-demo [data-action="paste-terminal-text"]',
    (_event, target) => {
      keyBarDemoOutput.value =
        (target as HTMLElement).dataset.action === 'copy-terminal-text'
          ? 'Copy → opens the terminal Copy sheet'
          : 'Paste → sends the clipboard to the terminal';
    },
  ),
);
// The rename dialog demo reports what each production action would do (HS2-2Q7KTX).
demoListeners.add(
  delegate(root, 'click', '[data-rename-demo-open]', (_event, target) => {
    const kind = (target as HTMLElement).dataset.renameDemoOpen === 'default' ? 'default' : 'renamed';
    renameDemoSession.value += 1;
    renameDemoTarget.value = { ...renameDemoTargets[kind], session: renameDemoSession.value };
  }),
);
demoListeners.add(
  delegate(root, 'click', '.terminal-rename-demo [data-action="reset-terminal-rename"]', () => {
    renameDemoOutput.value = `Reset to default → the tab shows ${renameDemoTarget.value?.defaultName ?? ''} again`;
    renameDemoTarget.value = undefined;
  }),
);
demoListeners.add(
  delegate(root, 'click', '.terminal-rename-demo [data-action="cancel-terminal-rename"]', () => {
    renameDemoOutput.value = 'Cancel → the name is unchanged';
    renameDemoTarget.value = undefined;
  }),
);
demoListeners.add(
  delegate(root, 'submit', '.terminal-rename-demo [data-action="rename-terminal-form"]', (event, target) => {
    event.preventDefault();
    const name = target.querySelector<HTMLInputElement>('[name="terminal-name"]')?.value.trim() ?? '';
    renameDemoOutput.value = `Rename → the tab shows ${name}`;
    renameDemoTarget.value = undefined;
  }),
);
demoListeners.add(
  delegate(root, 'wa-hide', '.terminal-rename-demo [data-terminal-rename-dialog]', () => {
    renameDemoTarget.value = undefined;
  }),
);
// The clipboard sheet demos drive the same open/close/selection behavior as production (HS2-FRB545).
demoListeners.add(
  delegate(root, 'click', '[data-clipboard-demo-open]', (_event, target) => {
    const kind = (target as HTMLElement).dataset.clipboardDemoOpen,
      generation = (clipboardDemoGeneration.value += 1);
    if (kind === 'copy')
      clipboardDemoCopy.value = { open: true, title: 'Build', generation, text: CLIPBOARD_DEMO_TEXT };
    else
      clipboardDemoPaste.value = {
        open: true,
        title: 'Build',
        generation,
        reason: kind === 'unavailable' ? 'unavailable' : 'denied',
      };
  }),
);
const closeClipboardDemo = () => {
  if (clipboardDemoCopy.value?.open) clipboardDemoCopy.value = { ...clipboardDemoCopy.value, open: false };
  if (clipboardDemoPaste.value?.open) clipboardDemoPaste.value = { ...clipboardDemoPaste.value, open: false };
};
demoListeners.add(
  delegate(root, 'click', '.terminal-clipboard-demo [data-action="confirm-terminal-copy"]', () => {
    const field = root.querySelector<HTMLTextAreaElement>(
      '.terminal-clipboard-demo textarea[name="terminal-copy-text"]',
    );
    if (!field) return;
    const { text, selection } = terminalCopySelection(field.value, field.selectionStart, field.selectionEnd);
    clipboardDemoOutput.value = terminalCopyMessage(text, selection);
    closeClipboardDemo();
  }),
);
demoListeners.add(
  delegate(root, 'submit', '.terminal-clipboard-demo [data-action="submit-terminal-paste"]', (event, target) => {
    event.preventDefault();
    const text = target.querySelector<HTMLTextAreaElement>('textarea[name="terminal-paste-text"]')?.value ?? '';
    clipboardDemoOutput.value = `Pasted → ${JSON.stringify(text.replace(/\r?\n/g, '\r')).slice(1, -1)}`;
    closeClipboardDemo();
  }),
);
demoListeners.add(
  delegate(
    root,
    'click',
    '.terminal-clipboard-demo [data-action="close-terminal-copy"], .terminal-clipboard-demo [data-action="cancel-terminal-paste"]',
    closeClipboardDemo,
  ),
);
demoListeners.add(delegate(root, 'wa-hide', '.terminal-clipboard-demo wa-dialog', closeClipboardDemo));
// The edit menu demo opens where the stage was pressed and reports the production action (HS2-KKP8YJ).
demoListeners.add(
  delegate(root, 'click', '[data-edit-menu-demo-open]', (_event, target) => {
    const box = target.getBoundingClientRect(),
      // A long-press on a word leaves a selection, so the menu leads with Copy (HS2-EYR96N).
      selection = (target as HTMLElement).dataset.editMenuDemoOpen === 'selection';
    editMenuDemo.value = {
      ...viewportSafeContextMenuPosition(box.left + box.width / 2, box.bottom, innerWidth, innerHeight, {
        width: 192,
        height: selection ? 144 : 96,
      }),
      selection,
    };
  }),
);
demoListeners.add(
  delegate(
    root,
    'click',
    '.terminal-edit-menu-demo [data-action="copy-terminal-selection"], .terminal-edit-menu-demo [data-action="copy-terminal-text"], .terminal-edit-menu-demo [data-action="paste-terminal-text"]',
    (_event, target) => {
      const action = (target as HTMLElement).dataset.action;
      editMenuDemoOutput.value =
        action === 'copy-terminal-selection'
          ? 'Copy → copies the selected terminal text'
          : action === 'copy-terminal-text'
            ? 'Copy Text… → opens the terminal Copy sheet'
            : 'Paste → sends the clipboard to the terminal';
      editMenuDemo.value = undefined;
    },
  ),
);
demoListeners.add(
  delegate(
    root,
    'click',
    '[data-component="project-close-dialog"] [data-action="select-project-close-resource"]',
    (_event, target) => {
      const key = target.getAttribute('data-item-id');
      if (key) projectCloseDemoSelection.value = key;
    },
  ),
);
// The drawer focus-mode text-size control cycles the demo's column fixture like production (HS2-01D4JP).
demoListeners.add(
  delegate(root, 'click', '.terminal-drawer-focus-demo [data-action="cycle-mobile-terminal-columns"]', () => {
    drawerFocusDemoColumns.value = nextMobileTerminalColumns(drawerFocusDemoColumns.value);
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.openTerminalContextMenu.selector, (event, target) => {
    event.preventDefault();
    event.stopPropagation();
    const box = target.getBoundingClientRect();
    showTerminalDashboardContextMenu(target as HTMLElement, box.right, box.bottom);
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.showTerminalVisibilityDemo.selector, showTerminalVisibilityDemo),
);
wireTerminalVisibilityTypeFilter(root, (types) => {
  terminalVisibilityDemoTypes.value = types;
});
demoListeners.add(
  delegate(root, 'wa-hide', '[data-terminal-visibility-dialog]', (event, target) => {
    if (event.target === target) closeTerminalVisibilityDemo();
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.selectTerminalVisibilityTab.selector, (_event, target) => {
    selectTerminalVisibilityDemoGroup((target as HTMLElement).dataset.itemId ?? 'default');
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.addTerminalVisibilityGroup.selector, () => {
    promptAddTerminalVisibilityDemoGroup();
    requestAnimationFrame(() =>
      root
        .querySelector<HTMLElement>('[data-terminal-visibility-name-dialog] [name="terminal-visibility-group-name"]')
        ?.focus(),
    );
  }),
);
demoListeners.add(
  delegate(root, 'contextmenu', '[data-visibility-group-id]', (event, target) => {
    event.preventDefault();
    showTerminalVisibilityDemoContextMenu(
      (target as HTMLElement).dataset.visibilityGroupId ?? '',
      (event as MouseEvent).clientX,
      (event as MouseEvent).clientY,
    );
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.renameTerminalVisibilityGroup.selector, () => {
    promptRenameTerminalVisibilityDemoGroup();
    requestAnimationFrame(() =>
      root
        .querySelector<HTMLElement>('[data-terminal-visibility-name-dialog] [name="terminal-visibility-group-name"]')
        ?.focus(),
    );
  }),
);
demoListeners.add(
  delegate(root, 'submit', DEMO_ACTIONS.submitTerminalVisibilityName.selector, (event, target) => {
    event.preventDefault();
    submitTerminalVisibilityDemoName(
      (target.querySelector('[name="terminal-visibility-group-name"]') as FormControl).value,
    );
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.cancelTerminalVisibilityName.selector, cancelTerminalVisibilityDemoName),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.removeTerminalVisibilityGroup.selector, () => {
    removeTerminalVisibilityDemoGroup();
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.toggleTerminalVisibility.selector, (_event, target) => {
    toggleTerminalVisibilityDemo((target as HTMLElement).dataset.itemId ?? '');
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.showAllTerminalsInGroup.selector, () => {
    setAllTerminalVisibilityDemo(true);
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.hideAllTerminalsInGroup.selector, () => {
    setAllTerminalVisibilityDemo(false);
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.transitionForward.selector, () => {
    transitionDirection.value = 'forward';
    transitionSide.value = 'b';
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.transitionBack.selector, () => {
    transitionDirection.value = 'backward';
    transitionSide.value = 'a';
  }),
);
demoListeners.add(
  delegate(root, 'change', '[data-settings="content-transition"] [name="transition-style"]', (_event, target) => {
    transitionStyle.value = (target as FormControl).value as typeof transitionStyle.value;
  }),
);
demoListeners.add(
  delegate(root, 'change', '[data-settings="content-transition"] [name="transition-side"]', (_event, target) => {
    const side = (target as FormControl).value as typeof transitionSide.value;
    transitionDirection.value = side === 'b' ? 'forward' : 'backward';
    transitionSide.value = side;
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.openRepositoryStatus.selector, () => {
    sidebarEvent.value = 'Repository status requested.';
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.selectRepositoryView.selector, (_event, target) => {
    repositoryDemoView.value = (target as HTMLElement).dataset.itemId as typeof repositoryDemoView.value;
    repositoryDemoDetailActive.value = true;
    repositoryDemoEvent.value = `${(target as HTMLElement).textContent.trim() || 'Repository view'} selected.`;
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.toggleRepositoryComparison.selector, () => {
    if (repositoryDemoComparison.value.active) {
      repositoryDemoComparison.value = { active: false, side: 'a' };
      return;
    }
    repositoryDemoView.value = 'commits';
    repositoryDemoComparison.value = { active: true, side: 'a' };
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.setRepositoryComparisonSide.selector, (_event, target) => {
    repositoryDemoComparison.value = {
      ...repositoryDemoComparison.value,
      side: (target as HTMLElement).dataset.comparisonSide as 'a' | 'b',
    };
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.selectRepositoryComparisonCommit.selector, (_event, target) => {
    const sha = (target as HTMLElement).dataset.commitSha!,
      current = repositoryDemoComparison.value;
    repositoryDemoComparison.value = current.side === 'a' ? { ...current, a: sha, side: 'b' } : { ...current, b: sha };
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.toggleCodeReviewCommit.selector, (_event, target) => {
    const sha = (target as HTMLElement).dataset.commitSha!;
    repositoryDemoExpandedCommits.value = repositoryDemoExpandedCommits.value.includes(sha)
      ? repositoryDemoExpandedCommits.value.filter((item) => item !== sha)
      : [...repositoryDemoExpandedCommits.value, sha];
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.refreshRepositoryStatus.selector, () => {
    repositoryDemoEvent.value = 'Repository status refreshed.';
  }),
);
const repositoryDemoFileSelector = '[data-action="select-repository-file"]';
const repositoryDemoFileMenuTrigger = '[data-action="open-repository-file-menu-trigger"]';
function openRepositoryDemoFileMenu(target: Element, x: number, y: number) {
  const element = target.closest<HTMLElement>(repositoryDemoFileSelector) ?? (target as HTMLElement);
  const path = element.dataset.itemId!;
  const ticket =
    (target as HTMLElement).dataset.fileMenuSource === 'ticket' ||
    Boolean(target.closest('[data-component="change-evidence-dialog"]'));
  const view = repositoryDemoView.value;
  const diff = ticket
    ? 'ticket'
    : view === 'staged' || view === 'unstaged'
      ? view
      : view === 'conflicted'
        ? 'unstaged'
        : undefined;
  const width = 232;
  const height = 226;
  const bounds = target.closest('.dialog-surface')?.getBoundingClientRect();
  const minimumX = bounds ? bounds.left + 8 : 8;
  const maximumX = Math.min(
    window.innerWidth - width - 8,
    bounds ? bounds.right - width - 8 : Number.POSITIVE_INFINITY,
  );
  const minimumY = bounds ? bounds.top + 8 : 8;
  const maximumY = Math.min(
    window.innerHeight - height - 8,
    bounds ? bounds.bottom - height - 8 : Number.POSITIVE_INFINITY,
  );
  repositoryDemoFileMenu.value = {
    path,
    absolutePath: `/work/hotsheet2/${path}`,
    diff,
    x: Math.max(minimumX, Math.min(x, maximumX)),
    y: Math.max(minimumY, Math.min(y, maximumY)),
    source: ticket ? 'ticket' : 'repository',
  };
  revealContextPopupMenu('repository-file');
}
demoListeners.add(
  delegate(root, 'dblclick', repositoryDemoFileSelector, (event, target) => {
    if ((event.target as Element).closest(repositoryDemoFileMenuTrigger)) return;
    repositoryDemoFileMenu.value = undefined;
    repositoryDemoEvent.value = `Would open ${(target as HTMLElement).dataset.itemId}.`;
  }),
);
demoListeners.add(
  delegate(root, 'click', repositoryDemoFileMenuTrigger, (event, target) => {
    event.preventDefault();
    event.stopImmediatePropagation();
    const box = target.getBoundingClientRect();
    openRepositoryDemoFileMenu(target, box.right, box.bottom);
  }),
);
demoListeners.add(
  delegate(root, 'keydown', repositoryDemoFileMenuTrigger, (event, target) => {
    const key = (event as KeyboardEvent).key;
    if (key !== 'Enter' && key !== ' ') return;
    event.preventDefault();
    event.stopImmediatePropagation();
    const box = target.getBoundingClientRect();
    openRepositoryDemoFileMenu(target, box.right, box.bottom);
  }),
);
demoListeners.add(
  delegate(root, 'contextmenu', repositoryDemoFileSelector, (event, target) => {
    event.preventDefault();
    const pointer = event as MouseEvent;
    openRepositoryDemoFileMenu(target, pointer.clientX, pointer.clientY);
  }),
);
demoListeners.add(
  delegate(root, 'click', '[data-repository-file-action]', (_event, target) => {
    const action = (target as HTMLElement).dataset.repositoryFileAction;
    const path = (target as HTMLElement).dataset.repositoryFilePath;
    repositoryDemoFileMenu.value = undefined;
    repositoryDemoEvent.value =
      action === 'show-diff' ? `Would review ${path} in Glassbox.` : `Would ${action} ${path}.`;
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.selectChangeEvidenceView.selector, (_event, target) => {
    changeEvidenceDemoView.value = (target as HTMLElement).dataset.itemId as typeof changeEvidenceDemoView.value;
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.openRepositoryReview.selector, (_event, target) => {
    repositoryDemoEvent.value = `Would open ${(target as HTMLElement).dataset.reviewMode} review in Glassbox.`;
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.addView.selector, () => {
    sidebarEvent.value = 'New view editor requested.';
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.selectView.selector, (_event, target) => {
    const id = (target as HTMLElement).dataset.itemId!;
    selectedViewId.value = id;
    sidebarEvent.value = `${sidebarViews.find((view) => view.id === id)?.label ?? 'View'} selected.`;
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.toggleCommandGroup.selector, () => {
    commandGroupExpanded.value = !commandGroupExpanded.value;
    sidebarEvent.value = commandGroupExpanded.value ? 'Command group expanded.' : 'Command group collapsed.';
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.toggleCommandSection.selector, (_event, target) => {
    const group = target.closest<HTMLElement>('[data-command-group]')?.dataset.commandGroup;
    if (!group) return;
    collapsedCommandGroups.value = collapsedCommandGroups.value.includes(group)
      ? collapsedCommandGroups.value.filter((item) => item !== group)
      : [...collapsedCommandGroups.value, group];
    sidebarEvent.value = collapsedCommandGroups.value.includes(group) ? `${group} collapsed.` : `${group} expanded.`;
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.runCommand.selector, (_event, target) => {
    const id = (target as HTMLElement).dataset.itemId!;
    runningCommandId.value = runningCommandId.value === id ? undefined : id;
    sidebarEvent.value = runningCommandId.value
      ? `${sidebarCommands.find((command) => command.id === id)?.label ?? 'Command'} started.`
      : 'Command stopped.';
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.toggleDrive.selector, () => {
    driveRunning.value = !driveRunning.value;
    sidebarEvent.value = driveRunning.value ? 'Codex drive started.' : 'Codex drive stopped.';
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.selectProjectTab.selector, (_event, target) => {
    selectProjectTab(target.closest<HTMLElement>('[data-tab-kind="project"]')!.dataset.projectId!);
  }),
);
demoListeners.add(
  delegate(root, 'change', 'wa-select[name="mobile-project"]', (_event, target) => {
    selectProjectTab((target as HTMLInputElement).value);
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.closeProjectTab.selector, (event, target) => {
    event.stopPropagation();
    closeProjectTab(target.closest<HTMLElement>('[data-tab-kind="project"]')!.dataset.projectId!);
  }),
);
demoListeners.add(
  delegate(root, 'contextmenu', DEMO_MARKERS.tabKindProject.selector, (event, target) => {
    event.preventDefault();
    const pointer = event as MouseEvent;
    tabContextMenu.value = {
      x: pointer.clientX,
      y: pointer.clientY,
      projectId: (target as HTMLElement).dataset.projectId!,
    };
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.projectTabContextAction.selector, (_event, target) => {
    const element = target as HTMLElement;
    const id = element.dataset.projectId!;
    if (element.dataset.tabAction === 'close') closeProjectTab(id);
    if (element.dataset.tabAction === 'close-others') closeOtherProjectTabs(id);
    if (element.dataset.tabAction === 'close-right') closeProjectTabsToRight(id);
    if (element.dataset.tabAction === 'close-all') closeAllProjectTabs();
    tabContextMenu.value = undefined;
  }),
);
demoListeners.add(
  delegate(root, 'click', '[data-action="add-project"], [data-action="choose-project"]', () => {
    addDemoProject();
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.toggleProjectSidebar.selector, () => {
    shellSidebarVisible.value = !shellSidebarVisible.value;
    shellEvent.value = shellSidebarVisible.value ? 'Project sidebar shown.' : 'Project sidebar hidden.';
  }),
);
demoListeners.add(
  delegate(
    root,
    'click',
    '[data-action="toggle-terminal-drawer"], [data-action="toggle-app-shell-demo-terminal-drawer"]',
    () => {
      shellTerminalDrawerVisible.value = !shellTerminalDrawerVisible.value;
      shellEvent.value = shellTerminalDrawerVisible.value ? 'Terminal drawer shown.' : 'Terminal drawer hidden.';
    },
  ),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.setShellMode.selector, (_event, target) => {
    shellStatsProjectName.value = undefined;
    shellMode.value = (target as HTMLElement).dataset.shellMode as typeof shellMode.value;
    workspaceSearchOpen.value = false;
    workspaceSearchHelpOpen.value = false;
    workspaceSearchModel.clear();
    shellEvent.value = shellMode.value === 'terminals' ? 'Workspace grid selected.' : 'Cross-project stats selected.';
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.openProjectStats.selector, () => {
    const name = projectTabs.value.find((tab) => tab.selected)?.name ?? 'Project';
    shellStatsProjectName.value = name;
    shellMode.value = 'stats';
    workspaceSearchOpen.value = false;
    workspaceSearchHelpOpen.value = false;
    workspaceSearchModel.clear();
    sidebarEvent.value = `${name} project statistics requested.`;
    shellEvent.value = `${name} project statistics selected.`;
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.toggleResizableCollapse.selector, () => {
    resizeDemoCollapsed.value = !resizeDemoCollapsed.value;
    shellEvent.value = resizeDemoCollapsed.value ? 'Horizontal region collapsed.' : 'Horizontal region restored.';
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.retryConnection.selector, () => {
    shellEvent.value = 'Connection retry requested.';
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.showConnectionDetails.selector, () => {
    shellEvent.value = 'Connection details requested.';
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.authenticateConnection.selector, () => {
    shellEvent.value = 'Authentication requested.';
  }),
);
demoListeners.add(
  delegate(root, 'pointerdown', DEMO_ACTIONS.resizeProjectSidebar.selector, (event) => {
    event.preventDefault();
    sidebarResizeDrag = {
      startY: (event as PointerEvent).clientY,
      startHeight: projectSidebarHeight.value,
    };
    document.body.dataset.resizingProjectSidebar = 'true';
  }),
);
demoListeners.add(
  delegate(root, 'keydown', DEMO_ACTIONS.resizeProjectSidebar.selector, (event) => {
    if ((event as KeyboardEvent).key !== 'ArrowUp' && (event as KeyboardEvent).key !== 'ArrowDown') return;
    event.preventDefault();
    projectSidebarHeight.value = clampProjectSidebarHeight(
      projectSidebarHeight.value + ((event as KeyboardEvent).key === 'ArrowDown' ? 24 : -24),
    );
    sidebarEvent.value = `Sidebar height ${projectSidebarHeight.value} pixels.`;
  }),
);
window.addEventListener('pointermove', (event) => {
  if (!sidebarResizeDrag) return;
  projectSidebarHeight.value = clampProjectSidebarHeight(
    sidebarResizeDrag.startHeight + event.clientY - sidebarResizeDrag.startY,
  );
});
window.addEventListener('pointerup', () => {
  if (!sidebarResizeDrag) return;
  sidebarResizeDrag = undefined;
  delete document.body.dataset.resizingProjectSidebar;
  sidebarEvent.value = `Sidebar height ${projectSidebarHeight.value} pixels.`;
});
window.addEventListener('keydown', (event) => {
  if (event.key === 'Escape') tabContextMenu.value = undefined;
});
demoListeners.add(
  delegate(root, 'click', '[data-app-loading-demo]', (_event, target) => {
    const state = (target as HTMLElement).dataset.appLoadingDemo;
    appLoadingDemoProgress.value =
      state === 'start'
        ? { completed: 0, total: 2 }
        : state === 'progress'
          ? { completed: 1, total: 2 }
          : state === 'complete'
            ? { completed: 2, total: 2 }
            : undefined;
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.resetSettings.selector, () => {
    if (selectedId.value === 'app-loading-indicator') appLoadingDemoProgress.value = undefined;
    if (selectedId.value === 'app-shell') resetAppShellDemo(root);
    if (selectedId.value === 'content-transition') resetContentTransitionDemo(root);
    if (selectedId.value === 'quick-ticket-composer') resetQuickTicketComposerDemo(root);
    if (selectedId.value === 'ticket-inspector') resetTicketInspectorDemo(root);
    if (selectedId.value === 'ai-conversation') {
      resetAIConversationDemo(root);
      openAIConversationDemo();
    }
    if (selectedId.value === 'tag-chip') resetTagChipDemo(root);
    if (selectedId.value === 'status-badge') resetStatusBadgeDemo(root);
    if (selectedId.value === 'confidence-badge') resetConfidenceBadgeDemo(root);
    if (selectedId.value === 'confidence-calibration') resetConfidenceCalibrationDemo(root);
    if (selectedId.value === 'ticket-row') resetTicketRowDemo(root);
    if (selectedId.value === 'repository-status-popover') resetRepositoryStatusDemo(root);
    if (selectedId.value === 'connection-details-dialog') resetConnectionDetailsDemo(root);
    if (selectedId.value === 'permission-request') resetPermissionRequestDemo(root);
    if (selectedId.value === 'command-run-dialog') resetCommandRunDialogDemo(root);
    if (selectedId.value === 'markdown-editor') resetMarkdownEditorDemo(root);
    if (selectedId.value === 'bulk-ticket-dialog') resetBulkTicketDialogDemo(root);
    if (selectedId.value === 'settings-workspace') resetSettingsWorkspaceDemo(root);
  }),
);
const openAIConversationDemo = () => {
  aiConversationDemoOpen.value = true;
  queueMicrotask(() => {
    root.querySelector<HTMLElement & { show?(): void }>('[data-component="ai-conversation"]')?.show?.();
  });
};
demoListeners.add(
  delegate(root, 'change', '[data-settings="ai-conversation"] [name="presentation"]', (_event, target) => {
    aiConversationPresentation.value = (target as FormControl).value as typeof aiConversationPresentation.value;
    openAIConversationDemo();
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.saveConversation.selector, () => {
    aiConversationSaveCount.value++;
  }),
);
demoListeners.add(
  delegate(root, 'change', '[data-settings="ai-conversation"] [name="scenario"]', (_event, target) => {
    aiConversationScenario.value = (target as FormControl).value as typeof aiConversationScenario.value;
    openAIConversationDemo();
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.openAiConversationDemo.selector, () => {
    openAIConversationDemo();
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.closeConversation.selector, () => {
    root.querySelector<HTMLElement & { hide?(): void }>('[data-component="ai-conversation"]')?.hide?.();
    aiConversationDemoOpen.value = false;
  }),
);
demoListeners.add(
  delegateCapture(root, 'wa-hide', DEMO_COMPONENTS.aiConversation.selector, () => {
    aiConversationDemoOpen.value = false;
  }),
);
demoListeners.add(
  delegate(root, 'input', DEMO_FIELDS.conversationDraft.selector, (_event, target) => {
    aiConversationDraft.value = (target as HTMLTextAreaElement).value;
  }),
);
demoListeners.add(
  delegate(root, 'submit', DEMO_ACTIONS.sendConversationTurn.selector, (event) => {
    event.preventDefault();
    aiConversationScenario.value = 'streaming';
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.stopConversation.selector, () => {
    aiConversationScenario.value = 'interrupted';
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.selectConversationProvider.selector, (_event, target) => {
    const id = target.closest<HTMLElement>('[data-value]')?.dataset.value;
    if (id) {
      aiConversationProvider.value = id;
      sidebarEvent.value = `Switched conversation provider to ${aiConversationProviderLabel(id)}.`;
    }
  }),
);
demoListeners.add(
  delegate(root, 'change', '[data-settings="repository-status-popover"] [name="scenario"]', (_event, target) => {
    repositoryDemoScenario.value = (target as FormControl).value as typeof repositoryDemoScenario.value;
  }),
);
demoListeners.add(
  delegate(root, 'change', '[data-settings="connection-details-dialog"] [name="scenario"]', (_event, target) => {
    connectionDetailsScenario.value = (target as FormControl).value as typeof connectionDetailsScenario.value;
  }),
);
demoListeners.add(
  delegate(root, 'change', '[data-demo-ticket-source-scenario] [name="scenario"]', (_event, target) => {
    ticketSourceScenario.value = (target as FormControl).value as TicketSourceScenario;
  }),
);
demoListeners.add(
  delegate(root, 'change', '[data-settings="permission-request"] [name]', (_event, target) => {
    const control = target as FormControl;
    switch (control.getAttribute('name')) {
      case 'presentation':
        permissionRequestSettings.presentation.value =
          control.value as typeof permissionRequestSettings.presentation.value;
        break;
      case 'variant':
        permissionRequestSettings.variant.value = control.value as typeof permissionRequestSettings.variant.value;
        break;
      case 'request':
        permissionRequestSettings.request.value = control.value as typeof permissionRequestSettings.request.value;
        break;
      case 'automation':
        resetPermissionRequestDemoCountdown();
        permissionRequestSettings.automation.value = control.value as typeof permissionRequestSettings.automation.value;
        break;
      case 'always-supported':
        permissionRequestSettings.alwaysSupported.value = control.checked;
        break;
      case 'explanation':
        permissionRequestSettings.explanation.value = control.checked;
        break;
      case null:
        break;
    }
  }),
);
demoListeners.add(
  delegate(root, 'click', '.workspace-component-demo [data-action="resolve-permission"]', (_event, target) => {
    const { requestKey, decision, scope } = (target as HTMLElement).dataset;
    if (!requestKey || (decision !== 'allow' && decision !== 'deny') || (scope !== 'once' && scope !== 'always'))
      return;
    resolveWorkspaceDemoPermission(requestKey, decision, scope);
  }),
);
demoListeners.add(
  delegate(root, 'click', '.workspace-component-demo [data-action="ignore-permission"]', (_event, target) => {
    const key = (target as HTMLElement).dataset.requestKey;
    if (key) ignoreWorkspaceDemoPermission(key);
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.resetWorkspaceNotifications.selector, () => {
    resetWorkspaceDemoNotifications();
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.cancelPermissionAutomation.selector, (event) => {
    event.stopImmediatePropagation();
    stopPermissionRequestDemoAutomation(root);
  }),
);
demoListeners.add(
  delegate(root, 'input', '[data-settings="tag-chip"] [name="label"]', (_event, target) => {
    tagChipSettings.label.value = (target as FormControl).value;
  }),
);
demoListeners.add(
  delegate(root, 'change', '[data-settings="tag-chip"] [name]', (_event, target) => {
    const control = target as FormControl;
    switch (control.getAttribute('name')) {
      case 'variant':
        tagChipSettings.variant.value = control.value as typeof tagChipSettings.variant.value;
        break;
      case 'appearance':
        tagChipSettings.appearance.value = control.value as typeof tagChipSettings.appearance.value;
        break;
      case 'size':
        tagChipSettings.size.value = control.value as typeof tagChipSettings.size.value;
        break;
      case 'removable':
        tagChipSettings.removable.value = control.checked;
        break;
      case 'pill':
        tagChipSettings.pill.value = control.checked;
        break;
      case 'disabled':
        tagChipSettings.disabled.value = control.checked;
        break;
      case null:
        break;
    }
  }),
);
demoListeners.add(
  delegate(root, 'click', `[aria-label="TagChip demo"] [data-action="${TAG_CHIP_REMOVE_ACTION}"]`, (_event, target) => {
    const chip = target.closest<HTMLElement>('[data-component="tag-chip"]');
    if (chip && chip.dataset.disabled !== 'true')
      tagChipSettings.event.value = `Remove requested for ${chip.dataset.tagId}`;
  }),
);
demoListeners.add(
  delegate(root, 'change', '[data-settings="app-shell"] [name]', (_event, target) => {
    const control = target as FormControl;
    if (control.getAttribute('name') === 'presentation')
      appShellSettings.presentation.value = control.value as typeof appShellSettings.presentation.value;
    if (control.getAttribute('name') === 'overlay-open') appShellSettings.overlayOpen.value = control.checked;
    if (control.getAttribute('name') === 'mobile-view-header') appShellSettings.mobileView.value = control.checked;
  }),
);
demoListeners.add(
  delegate(root, 'change', '[aria-label="AppShell demo"] wa-select[name="mobile-view"]', (_event, target) => {
    const value = (target as FormControl).value;
    appShellSettings.mobileViewValue.value = value;
    shellEvent.value = `${value === 'all' ? 'Queue' : value.charAt(0).toUpperCase() + value.slice(1)} view selected.`;
  }),
);
demoListeners.add(
  delegate(root, 'click', '[aria-label="AppShell demo"] [data-action="open-empty-trash"]', () => {
    shellEvent.value = 'Empty Trash confirmation requested.';
  }),
);
demoListeners.add(
  delegate(root, 'change', '[data-settings="status-badge"] [name]', (_event, target) => {
    const control = target as FormControl;
    if (control.getAttribute('name') === 'status')
      statusBadgeSettings.status.value = control.value as typeof statusBadgeSettings.status.value;
    if (control.getAttribute('name') === 'started-phase')
      statusBadgeSettings.startedPhase.value = control.value as typeof statusBadgeSettings.startedPhase.value;
    if (control.getAttribute('name') === 'appearance')
      statusBadgeSettings.appearance.value = control.value as typeof statusBadgeSettings.appearance.value;
    if (control.getAttribute('name') === 'weight')
      statusBadgeSettings.weight.value = control.value as typeof statusBadgeSettings.weight.value;
    if (control.getAttribute('name') === 'show-icon') statusBadgeSettings.showIcon.value = control.checked;
    if (control.getAttribute('name') === 'compact') statusBadgeSettings.compact.value = control.checked;
  }),
);
demoListeners.add(
  delegate(root, 'change', '[data-settings="confidence-calibration"] [name]', (_event, target) => {
    const control = target as FormControl;
    if (control.getAttribute('name') === 'state') confidenceCalibrationSettings.state.value = control.value;
  }),
);
demoListeners.add(
  delegate(root, 'change', '[data-settings="confidence-badge"] [name]', (_event, target) => {
    const control = target as FormControl;
    if (control.getAttribute('name') === 'value') confidenceBadgeSettings.value.value = control.value;
    if (control.getAttribute('name') === 'appearance')
      confidenceBadgeSettings.appearance.value = control.value as typeof confidenceBadgeSettings.appearance.value;
  }),
);
demoListeners.add(wireWorkspaceOverflowKeyboard(root));
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.setToolbarGroupDemoMode.selector, (_event, target) => {
    toolbarGroupDemoMode.value = (target as HTMLElement).dataset.segmentValue as typeof toolbarGroupDemoMode.value;
  }),
);
function openSelectedDemoTicketActions(target: Element): void {
  const selected = collectionTickets.value.find((ticket) => ticket.selected);
  if (!selected) return;
  const rect = target.getBoundingClientRect();
  contextMenu.value = {
    ...viewportSafeContextMenuPosition(rect.right - 232, rect.bottom, innerWidth, innerHeight, {
      width: 232,
      height: 382,
    }),
    ticketSlug: selected.slug,
  };
}
demoListeners.add(delegate(root, 'click', DEMO_ACTIONS.toggleSelectedUpNext.selector, toggleWorkspaceDemoUpNext));
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.openSelectedTicketActions.selector, (_event, target) => {
    openSelectedDemoTicketActions(target);
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.setViewMode.selector, (_event, target) => {
    const metadata = (target as HTMLElement).dataset;
    workspaceMode.value = (metadata.segmentValue ?? metadata.viewMode) as typeof workspaceMode.value;
    if (workspaceMode.value === 'settings') {
      workspaceSearchOpen.value = false;
      workspaceSearchHelpOpen.value = false;
      workspaceSearchModel.clear();
    }
    recordCollectionEvent(
      `${workspaceMode.value === 'list' ? 'List' : workspaceMode.value === 'board' ? 'Columns' : workspaceMode.value === 'notifications' ? 'Notifications' : 'Settings'} view selected`,
    );
  }),
);
demoListeners.add(
  delegate(root, 'click', 'wa-select[name="workspace-sort"] wa-option', (_event, target) => {
    const next = nextWorkspaceSort(
      workspaceSort.value,
      workspaceSortDirection.value,
      (target as FormControl).value as typeof workspaceSort.value,
    );
    workspaceSort.value = next.sort;
    workspaceSortDirection.value = next.direction;
    recordCollectionEvent(`Sorted by ${workspaceSort.value}, ${workspaceSortDirection.value}`);
  }),
);
demoListeners.add(
  delegate(root, 'wa-select', '[data-workspace-overflow]', (event) => {
    const item = (event as CustomEvent<{ item: HTMLElement }>).detail.item;
    const action = item.dataset.workspaceOverflowAction;
    if (action === 'toggle-selected-up-next') {
      toggleWorkspaceDemoUpNext();
      return;
    }
    if (action === 'open-selected-ticket-actions') {
      openSelectedDemoTicketActions(item);
      return;
    }
    if (action === 'open-workspace-search') {
      workspaceSearchOpen.value = true;
      queueMicrotask(() => focusWorkspaceSearch(root));
      return;
    }
    if (action === 'set-view-mode') {
      workspaceMode.value = item.dataset.viewMode as typeof workspaceMode.value;
      if (workspaceMode.value === 'settings') {
        workspaceSearchOpen.value = false;
        workspaceSearchHelpOpen.value = false;
        workspaceSearchModel.clear();
      }
      recordCollectionEvent(
        `${workspaceMode.value === 'list' ? 'List' : workspaceMode.value === 'board' ? 'Columns' : workspaceMode.value === 'notifications' ? 'Notifications' : 'Settings'} view selected`,
      );
      return;
    }
    if (action !== 'set-workspace-sort') return;
    const selected = item.dataset.workspaceSort as typeof workspaceSort.value | undefined;
    if (!selected) return;
    const next = nextWorkspaceSort(workspaceSort.value, workspaceSortDirection.value, selected);
    workspaceSort.value = next.sort;
    workspaceSortDirection.value = next.direction;
    recordCollectionEvent(`Sorted by ${workspaceSort.value}, ${workspaceSortDirection.value}`);
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.toggleFavorite.selector, () => {
    recordCollectionEvent('View favorite toggled');
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.moreWorkspaceActions.selector, () => {
    recordCollectionEvent('Workspace actions requested');
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.expandTicketComposer.selector, (_event, target) => {
    if (document.activeElement !== target) (target as HTMLElement).focus({ preventScroll: true });
    composerExpanded.value = true;
    requestAnimationFrame(() => requestAnimationFrame(() => showQuickTicketComposer(root)));
  }),
);
demoListeners.add(
  delegateCapture(root, 'wa-after-hide', DEMO_COMPONENTS.quickTicketComposer.selector, (event, target) => {
    if (event.target !== target || !composerExpanded.value) return;
    composerExpanded.value = false;
    composerTitle.value = '';
    composerDetails.value = '';
    composerUpNext.value = false;
    composerSourcePick.value = undefined;
    composerAttachments.value = [];
    recordCollectionEvent('Ticket creation cancelled');
  }),
);
demoListeners.add(
  delegate(root, 'input', DEMO_FIELDS.newTicketTitle.selector, (_event, target) => {
    composerTitle.value = (target as FormControl).value;
  }),
);
demoListeners.add(
  delegate(root, 'input', DEMO_FIELDS.newTicketDetails.selector, (_event, target) => {
    composerDetails.value = (target as HTMLTextAreaElement).value;
  }),
);
demoListeners.add(
  delegate(root, 'change', DEMO_FIELDS.newTicketCategory.selector, (_event, target) => {
    composerCategory.value = (target as FormControl).value;
  }),
);
demoListeners.add(
  delegate(root, 'change', DEMO_FIELDS.newTicketSource.selector, (_event, target) => {
    composerSourcePick.value = (target as FormControl).value;
  }),
);
demoListeners.add(
  delegate(root, 'change', '[data-settings="markdown-editor"] [name="markdown-appearance"]', (_event, target) => {
    markdownAppearance.value = (target as FormControl).value as MarkdownEditorAppearanceDemo;
  }),
);
demoListeners.add(
  delegate(root, 'change', '[data-settings="markdown-editor"] [name="markdown-inset"]', (_event, target) => {
    markdownInset.value = (target as FormControl).value as MarkdownEditorInsetDemo;
  }),
);
demoListeners.add(
  delegate(root, 'change', '[data-settings="bulk-ticket-dialog"] [name="bulk-scenario"]', (_event, target) => {
    setBulkTicketDialogScenario((target as FormControl).value as BulkTicketDialogScenario);
  }),
);
demoListeners.add(delegate(root, 'click', DEMO_ACTIONS.openBulkTicketDialogDemo.selector, openBulkTicketDialogDemo));
// Fixture stand-ins for the production selection handlers (`interactions/ticket-selection.ts`): every
// rendered BulkTicketDialog action closes the dialog and reports what production would request.
demoListeners.add(
  delegate(root, 'click', TICKET_SELECTION_ACTIONS.cancelBulkTicketAction.selector, () => {
    closeBulkTicketDialogDemo('Cancelled; the selection is unchanged.');
  }),
);
demoListeners.add(
  delegate(root, 'click', TICKET_SELECTION_ACTIONS.chooseBulkTag.selector, (_event, target) => {
    const input = root.querySelector<FormControl>('[name="bulk-ticket-tag"]');
    if (!input) return;
    input.value = (target as HTMLElement).dataset.tag ?? '';
    input.focus();
  }),
);
demoListeners.add(
  delegate(root, 'submit', TICKET_SELECTION_ACTIONS.submitBulkTag.selector, (event, target) => {
    event.preventDefault();
    const tag = target.querySelector<FormControl>('[name="bulk-ticket-tag"]')?.value.trim() ?? '',
      adding = (target as HTMLElement).dataset.tagMode === 'add';
    closeBulkTicketDialogDemo(
      `${adding ? 'Add' : 'Remove'} tag “${tag}” ${adding ? 'to' : 'from'} 5 tickets requested.`,
    );
  }),
);
demoListeners.add(
  delegate(root, 'click', TICKET_SELECTION_ACTIONS.confirmBulkDelete.selector, () => {
    closeBulkTicketDialogDemo('Delete 5 tickets requested.');
  }),
);
demoListeners.add(
  delegate(root, 'click', TICKET_SELECTION_ACTIONS.confirmEmptyTrash.selector, () => {
    closeBulkTicketDialogDemo('Empty Trash requested for 5 tickets.');
  }),
);
demoListeners.add(
  delegate(
    root,
    'wa-hide',
    '[data-component="bulk-tag-dialog"], [data-component="bulk-delete-dialog"], [data-component="empty-trash-dialog"]',
    (event, target) => {
      if (event.target !== target) return;
      closeBulkTicketDialogDemo('Dismissed; the selection is unchanged.');
    },
  ),
);
demoListeners.add(
  delegate(root, 'change', '[data-settings="settings-workspace"] [name="workspace-category"]', (_event, target) => {
    settingsWorkspaceSettings.category.value = (target as FormControl).value as SettingsCategory;
  }),
);
demoListeners.add(
  delegate(
    root,
    'change',
    '[data-settings="settings-workspace"] [name="workspace-permission-action"]',
    (_event, target) => {
      settingsWorkspaceSettings.permissionAction.value = (target as FormControl).value as PermissionAutomationAction;
    },
  ),
);
demoListeners.add(
  delegate(root, 'change', '[data-settings="ticket-inspector"] [name="inspector-live-claim"]', (_event, target) => {
    inspectorLiveClaim.value = (target as FormControl).value as InspectorLiveClaimDemo;
  }),
);
demoListeners.add(
  delegate(
    root,
    'change',
    '[data-settings="quick-ticket-composer"] [name="composer-source-count"]',
    (_event, target) => {
      composerMultipleSources.value = (target as FormControl).value === 'several';
    },
  ),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.toggleNewTicketUpNext.selector, () => {
    composerUpNext.value = !composerUpNext.value;
  }),
);
let demoAttachmentSequence = 0;
demoListeners.add(
  delegate(root, 'change', 'input[name="new-ticket-attachments"]', (_event, target) => {
    const input = target as HTMLInputElement;
    composerAttachments.value = [
      ...composerAttachments.value,
      ...Array.from(input.files ?? [], (file) => ({ id: `demo-${demoAttachmentSequence++}`, name: file.name })),
    ];
    input.value = '';
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.removeNewTicketAttachment.selector, (_event, target) => {
    const id = (target as HTMLElement).dataset.pendingAttachmentId;
    composerAttachments.value = composerAttachments.value.filter((item) => item.id !== id);
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.clearNewTicketAttachments.selector, () => {
    composerAttachments.value = [];
    recordCollectionEvent('Staged attachments removed');
  }),
);
demoListeners.add(
  delegate(root, 'submit', DEMO_ACTIONS.createTicketForm.selector, (event) => {
    event.preventDefault();
    if (!createDemoTicket())
      recordCollectionEvent(
        composerTitle.value.trim() ? 'Remove the staged attachments or choose another source' : 'Enter a ticket title',
      );
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.setInspectorTab.selector, (_event, target) => {
    const tab = (target as HTMLElement).dataset.tabId as typeof inspectorTab.value;
    if (selectedId.value === 'ticket-reader') readerTab.value = tab;
    else inspectorTab.value = tab;
  }),
);
// The right rail's standard toggle (HS2-QQW6CT) both hides and shows the inspector.
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.toggleTicketInspector.selector, () => {
    inspectorOpen.value = !inspectorOpen.value;
    recordCollectionEvent(inspectorOpen.value ? 'Inspector opened' : 'Inspector closed');
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.openCodeReview.selector, (_event, target) => {
    const item = target as HTMLElement;
    recordCollectionEvent(
      item.dataset.reviewMode === 'range' ? 'Commit range opened in Glassbox' : 'Commit opened in Glassbox',
    );
  }),
);
demoListeners.add(
  delegate(root, 'change', DEMO_FIELDS.inspectorCategory.selector, (_event, target) => {
    inspectorCategory.value = (target as FormControl).value;
  }),
);
demoListeners.add(
  delegate(root, 'change', DEMO_FIELDS.inspectorPriority.selector, (_event, target) => {
    inspectorPriority.value = (target as FormControl).value as typeof inspectorPriority.value;
  }),
);
demoListeners.add(
  delegate(root, 'click', INSPECTOR_AND_EDITOR_ACTIONS.openInspectorStatusMenu.selector, (_event, target) => {
    const menu = target.closest('.ticket-status-menu')?.querySelector<PopupMenuElement>('[data-inspector-status-menu]');
    if (!menu) return;
    const rect = target.getBoundingClientRect();
    openPopupMenuAt(menu, rect.left, statusMenuAnchorY(rect.bottom, window.innerWidth, window.innerHeight));
  }),
);
demoListeners.add(
  delegate(root, 'click', INSPECTOR_AND_EDITOR_ACTIONS.setInspectorStatus.selector, (_event, target) => {
    const value = (target as HTMLElement).dataset.ticketStatus;
    if (!value) return;
    if (target.closest('.metadata-control-demo')) {
      statusMenuDemoStatus.value = value as typeof statusMenuDemoStatus.value;
      statusMenuDemoStartedPhase.value = '';
    } else {
      inspectorStatus.value = value as typeof inspectorStatus.value;
      inspectorStartedPhase.value = undefined;
    }
  }),
);
demoListeners.add(
  delegate(root, 'click', INSPECTOR_AND_EDITOR_ACTIONS.setInspectorStartedPhase.selector, (_event, target) => {
    const phase = (target as HTMLElement).dataset.startedPhase;
    if (phase === undefined) return;
    if (target.closest('.metadata-control-demo')) {
      statusMenuDemoStatus.value = 'started';
      statusMenuDemoStartedPhase.value = phase as typeof statusMenuDemoStartedPhase.value;
    } else {
      inspectorStatus.value = 'started';
      inspectorStartedPhase.value = (phase || undefined) as typeof inspectorStartedPhase.value;
    }
  }),
);
// The reader demo edits its own ticket's title; every other surface edits the inspector demo ticket's
// (HS2-0VFPD5). Each surface projects its own title, draft, and editing signals.
const titleSurface = (target: Element) =>
  target.closest('[data-component="ticket-reader"]')
    ? { title: readerTitle, draft: readerTitleDraft, editing: readerTitleEditing, autosave: readerTitleAutosave }
    : {
        title: inspectorTitle,
        draft: inspectorTitleDraft,
        editing: inspectorTitleEditing,
        autosave: titleAutosave,
      };
const beginTitleEdit = (target: Element) => {
  const surface = titleSurface(target),
    host = target.closest('[data-component="ticket-reader"], [data-component="ticket-inspector-header"]') ?? root;
  surface.draft.value = surface.title.value;
  surface.editing.value = true;
  queueMicrotask(() => host.querySelector<HTMLElement>('[name="ticket-title"]')?.focus());
};
demoListeners.add(
  delegate(root, 'dblclick', DEMO_ACTIONS.editTicketTitle.selector, (_event, target) => {
    beginTitleEdit(target);
  }),
);
demoListeners.add(
  delegate(root, 'keydown', DEMO_ACTIONS.editTicketTitle.selector, (event, target) => {
    if (!['Enter', ' '].includes((event as KeyboardEvent).key)) return;
    event.preventDefault();
    beginTitleEdit(target);
  }),
);
demoListeners.add(
  delegate(root, 'input', DEMO_FIELDS.ticketTitle.selector, (_event, target) => {
    const surface = titleSurface(target);
    // The wrapping title editor keeps a title on one line: pasted breaks collapse (HS2-98ZVPE).
    surface.draft.value = normalizeTicketTitleField(target as HTMLTextAreaElement);
    if (surface.draft.value.trim()) surface.autosave.schedule(surface.draft.value);
  }),
);
demoListeners.add(
  delegate(root, 'keydown', DEMO_FIELDS.ticketTitle.selector, (event, target) => {
    if (!ticketTitleKeyFinishesEdit(event as KeyboardEvent)) return;
    event.preventDefault();
    (target as HTMLTextAreaElement).blur();
  }),
);
demoListeners.add(
  delegate(root, 'focusout', DEMO_FIELDS.ticketTitle.selector, (_event, target) => {
    const surface = titleSurface(target);
    if (!surface.draft.value.trim()) return;
    void surface.autosave.flush().then(() => {
      surface.editing.value = false;
    });
  }),
);
const addInspectorTag = (target: HTMLInputElement) => {
  inspectorTags.value = addTicketTag(inspectorTags.value, target.value);
  target.value = '';
  tagsAutosave.schedule(inspectorTags.value);
};
demoListeners.add(
  delegate(root, 'keydown', DEMO_FIELDS.ticketTagInput.selector, (event, target) => {
    if (!['Enter', ','].includes((event as KeyboardEvent).key)) return;
    event.preventDefault();
    addInspectorTag(target as HTMLInputElement);
  }),
);
demoListeners.add(
  delegate(root, 'focusout', DEMO_FIELDS.ticketTagInput.selector, (_event, target) => {
    if ((target as HTMLInputElement).value.trim()) addInspectorTag(target as HTMLInputElement);
    void tagsAutosave.flush();
  }),
);
demoListeners.add(
  delegate(root, 'click', `[data-action="${TAG_CHIP_REMOVE_ACTION}"]`, (_event, target) => {
    const tag = target.closest<HTMLElement>('[data-component="tag-chip"]')?.dataset.tagId;
    if (!tag) return;
    inspectorTags.value = removeTicketTag(inspectorTags.value, tag);
    tagsAutosave.schedule(inspectorTags.value);
  }),
);
const beginBlockedReasonEdit = () => {
  inspectorBlockedReasonDraft.value = inspectorBlockedReason.value;
  inspectorBlockedReasonEditing.value = true;
  queueMicrotask(() => root.querySelector<HTMLElement>('[name="blocked-reason"]')?.focus());
};
demoListeners.add(delegate(root, 'click', DEMO_ACTIONS.editBlockedReason.selector, beginBlockedReasonEdit));
demoListeners.add(delegate(root, 'dblclick', DEMO_MARKERS.editBlockedReason.selector, beginBlockedReasonEdit));
demoListeners.add(
  delegate(root, 'keydown', DEMO_MARKERS.editBlockedReason.selector, (event) => {
    if (!['Enter', ' '].includes((event as KeyboardEvent).key)) return;
    event.preventDefault();
    beginBlockedReasonEdit();
  }),
);
demoListeners.add(
  delegate(root, 'input', DEMO_FIELDS.blockedReason.selector, (_event, target) => {
    inspectorBlockedReasonDraft.value = (target as FormControl).value;
    blockedReasonAutosave.schedule(inspectorBlockedReasonDraft.value);
  }),
);
demoListeners.add(
  delegate(root, 'focusout', DEMO_FIELDS.blockedReason.selector, () => {
    void blockedReasonAutosave.flush().then(() => {
      inspectorBlockedReasonEditing.value = false;
      recordCollectionEvent('Blocked reason autosaved');
    });
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.toggleInspectorUpNext.selector, () => {
    const ticket = collectionTickets.value.find((item) => item.selected) ?? collectionTickets.value[0];
    toggleCollectionTicketUpNext(ticket.slug);
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.addTicketNote.selector, () => {
    recordCollectionEvent('Note composer requested');
  }),
);
demoListeners.add(
  delegate(root, 'input', DEMO_FIELDS.newNoteBody.selector, (_event, target) => {
    noteComposerValue.value = (target as FormControl).value;
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.cancelNewNote.selector, () => {
    noteComposerValue.value = '';
    recordCollectionEvent('New note cancelled');
  }),
);
demoListeners.add(
  delegate(root, 'submit', DEMO_ACTIONS.createNoteForm.selector, (event) => {
    event.preventDefault();
    if (noteComposerValue.value.trim()) recordCollectionEvent('New note submitted');
  }),
);
const beginNoteEdit = (id: string) => {
  editingNoteId.value = id;
  noteDraft.value =
    readerNotes.value.find((note) => note.id === id)?.body ??
    noteDemoNotes.value.find((note) => note.id === id)?.body ??
    '';
  queueMicrotask(() => root.querySelector<HTMLElement>(`[name="note-body"][data-note-id="${id}"]`)?.focus());
};
demoListeners.add(
  delegate(root, 'click', DEMO_MARKERS.editOnClick.selector, (event, target) => {
    if (!clickBeginsMarkdownEdit(event as MouseEvent, target)) return;
    beginNoteEdit((target as HTMLElement).closest<HTMLElement>('[data-note-id]')!.dataset.noteId!);
  }),
);
demoListeners.add(
  delegate(root, 'keydown', DEMO_MARKERS.editOnClick.selector, (event, target) => {
    if (!keyBeginsMarkdownEdit(event as KeyboardEvent, target)) return;
    event.preventDefault();
    beginNoteEdit((target as HTMLElement).closest<HTMLElement>('[data-note-id]')!.dataset.noteId!);
  }),
);
demoListeners.add(
  delegate(root, 'input', DEMO_FIELDS.noteBody.selector, (_event, target) => {
    editingNoteId.value = (target as HTMLElement).dataset.noteId;
    noteDraft.value = (target as FormControl).value;
    if ((target as HTMLElement).dataset.noteResponse !== 'true' && editingNoteId.value)
      noteAutosave.schedule({ id: editingNoteId.value, value: noteDraft.value });
  }),
);
demoListeners.add(
  delegate(root, 'focusout', DEMO_FIELDS.noteBody.selector, (_event, target) => {
    if ((target as HTMLElement).dataset.noteResponse === 'true') return;
    void noteAutosave.flush().then(() => {
      editingNoteId.value = undefined;
      noteDraft.value = '';
      recordCollectionEvent('Note autosaved');
    });
  }),
);
let demoFeedbackChoiceAnchor: string | undefined = 'choice-1';
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.toggleFeedbackChoice.selector, (event, target) => {
    if ((event.target as Element).closest('a,[data-action="open-attachment-gallery"]')) return;
    const noteId = (target as HTMLElement).dataset.noteId!,
      choiceId = (target as HTMLElement).dataset.choiceId!;
    const note = readerNotes.value.find((item) => item.id === noteId),
      group = note && parseFeedbackChoices(note.body);
    if (!group) return;
    const pointer = event as MouseEvent,
      next = updateFeedbackChoiceSelection(
        group.choices.map((choice) => choice.id),
        readerFeedbackChoiceSelections.value[noteId] ?? [],
        choiceId,
        demoFeedbackChoiceAnchor,
        { additive: pointer.metaKey || pointer.ctrlKey, range: pointer.shiftKey },
      );
    readerFeedbackChoiceSelections.value = { ...readerFeedbackChoiceSelections.value, [noteId]: next.selected };
    demoFeedbackChoiceAnchor = next.anchor;
    recordCollectionEvent(`${next.selected.length} feedback choice${next.selected.length === 1 ? '' : 's'} selected`);
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.saveNoteEdit.selector, (_event, target) => {
    const id = editingNoteId.value ?? (target as HTMLElement).dataset.noteId;
    if (id) {
      const original = readerNotes.value.find((note) => note.id === id);
      if ((target as HTMLElement).dataset.noteResponse === 'true')
        readerNotes.value = [
          ...readerNotes.value,
          {
            id: `${id}-response`,
            kind: 'regular',
            author: 'You',
            time: 'Now',
            body: noteDraft.value,
          },
        ];
      else
        readerNotes.value = readerNotes.value.map((note) =>
          note.id === id
            ? {
                ...note,
                kind: original?.kind === 'feedback_draft' ? ('regular' as const) : note.kind,
                body: noteDraft.value,
              }
            : note,
        );
      noteDemoNotes.value = noteDemoNotes.value.map((note) =>
        note.id === id ? { ...note, body: noteDraft.value } : note,
      );
    }
    editingNoteId.value = undefined;
    noteDraft.value = '';
    recordCollectionEvent('Note edit saved');
  }),
);
function openDemoTicketReader(): void {
  readerDialogOpen.value = false;
  selectDemo('ticket-reader');
  requestAnimationFrame(() => {
    showTicketReaderDialog(root, 'ux-demo-ticket-reader');
    readerDialogOpen.value = true;
  });
}
demoListeners.add(
  delegate(root, 'click', '[data-action="open-ticket-reader"], [data-action="respond-to-feedback"]', () => {
    recordCollectionEvent('Ticket reader requested');
    openDemoTicketReader();
  }),
);
demoListeners.add(
  delegate(root, 'input', DEMO_FIELDS.markdownSource.selector, (_event, target) => {
    markdownValue.value = (target as FormControl).value;
    markdownEvent.value = 'Saving changes…';
    markdownAutosave.schedule(markdownValue.value);
  }),
);
demoListeners.add(
  delegate(root, 'focusout', DEMO_FIELDS.markdownSource.selector, (event, target) => {
    const next = (event as FocusEvent).relatedTarget;
    if (next instanceof Node && target.closest('[data-component="markdown-editor"]')?.contains(next)) return;
    setTimeout(
      () =>
        void markdownAutosave.flush().then(() => {
          markdownMode.value = 'preview';
        }),
      0,
    );
  }),
);
demoListeners.add(
  delegateCapture(root, 'mousedown', '*', (event) => {
    if (repeatPressWouldLeaveNewEditor(event as MouseEvent, document.activeElement)) event.preventDefault();
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.editMarkdown.selector, (event, target) => {
    if (!clickBeginsMarkdownEdit(event as MouseEvent, target)) return;
    markdownMode.value = 'write';
    queueMicrotask(() => root.querySelector<HTMLElement>('[name="markdown-source"]')?.focus());
  }),
);
demoListeners.add(
  delegate(root, 'keydown', DEMO_ACTIONS.editMarkdown.selector, (event, target) => {
    if (!keyBeginsMarkdownEdit(event as KeyboardEvent, target)) return;
    event.preventDefault();
    markdownMode.value = 'write';
    queueMicrotask(() => root.querySelector<HTMLElement>('[name="markdown-source"]')?.focus());
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.toggleMarkdownExpanded.selector, () => {
    markdownExpanded.value = !markdownExpanded.value;
    markdownEvent.value = markdownExpanded.value ? 'Expanded editor opened.' : 'Inline editor restored.';
  }),
);
demoListeners.add(
  delegateCapture(root, 'wa-after-hide', DEMO_COMPONENTS.ticketReader.selector, () => {
    readerDialogOpen.value = false;
    selectDemo('ticket-info-panel');
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.toggleReaderTextSize.selector, () => {
    readerLargeText.value = !readerLargeText.value;
  }),
);
const addMockAttachments = (files: FileList | File[], target: HTMLElement) => {
  const added = Array.from(files).map((file, index) => ({
    id: `added-${Date.now()}-${index}`,
    name: file.name,
  }));
  if (selectedId.value === 'ticket-reader' || target.closest('[data-component="ticket-attachments"]'))
    readerAttachments.value = [...readerAttachments.value, ...added];
  recordCollectionEvent(
    `${added.length} attachment${added.length === 1 ? '' : 's'} added to ${target.closest<HTMLElement>('[data-ticket-slug]')?.dataset.ticketSlug ?? 'ticket'}`,
  );
};
demoListeners.add(
  delegate(root, 'change', 'input[name="ticket-attachments"]', (_event, target) => {
    const input = target as HTMLInputElement;
    if (input.files?.length) addMockAttachments(input.files, input);
    input.value = '';
  }),
);
demoListeners.add(
  delegate(root, 'dragover', DEMO_MARKERS.attachmentDropTarget.selector, (event, target) => {
    event.preventDefault();
    (target as HTMLElement).dataset.draggingAttachment = 'true';
  }),
);
demoListeners.add(
  delegate(root, 'dragleave', DEMO_MARKERS.attachmentDropTarget.selector, (_event, target) => {
    delete (target as HTMLElement).dataset.draggingAttachment;
  }),
);
demoListeners.add(
  delegate(root, 'drop', DEMO_MARKERS.attachmentDropTarget.selector, (event, target) => {
    event.preventDefault();
    const element = target as HTMLElement;
    delete element.dataset.draggingAttachment;
    const files = (event as DragEvent).dataTransfer?.files;
    if (files?.length) addMockAttachments(files, element);
  }),
);
demoListeners.add(
  delegate(root, 'input', '[data-settings="ticket-list-row"] wa-input', (_event, target) => {
    const control = target as FormControl;
    if (control.getAttribute('name') === 'title') ticketRowSettings.title.value = control.value;
    if (control.getAttribute('name') === 'category') ticketRowSettings.category.value = control.value;
    if (control.getAttribute('name') === 'tags') ticketRowSettings.tags.value = control.value;
    if (control.getAttribute('name') === 'agent') ticketRowSettings.agentName.value = control.value;
    if (control.getAttribute('name') === 'updated') ticketRowSettings.updatedLabel.value = control.value;
  }),
);
demoListeners.add(
  delegate(root, 'change', '[data-settings="ticket-list-row"] [name]', (_event, target) => {
    const control = target as FormControl;
    switch (control.getAttribute('name')) {
      case 'status':
        ticketRowSettings.status.value = control.value as typeof ticketRowSettings.status.value;
        break;
      case 'started-phase':
        ticketRowSettings.startedPhase.value = control.value as typeof ticketRowSettings.startedPhase.value;
        break;
      case 'priority':
        ticketRowSettings.priority.value = control.value as typeof ticketRowSettings.priority.value;
        break;
      case 'category-icon':
        ticketRowSettings.categoryIcon.value = control.value;
        break;
      case 'category-color':
        ticketRowSettings.categoryColor.value = control.value;
        break;
      case 'up-next':
        ticketRowSettings.upNext.value = control.checked;
        break;
      case 'blocked':
        ticketRowSettings.blocked.value = control.checked;
        break;
      case 'needs-review':
        ticketRowSettings.needsReview.value = control.checked;
        break;
      case 'feedback-needed':
        ticketRowSettings.feedbackNeeded.value = control.checked;
        break;
      case 'selected':
        ticketRowSettings.selected.value = control.checked;
        break;
      case 'busy':
        ticketRowSettings.busy.value = control.checked;
        break;
      case 'show-source-icon':
        ticketRowSettings.showSourceIcon.value = control.checked;
        break;
      case 'claim-eta':
        ticketRowSettings.claimEta.value = control.value as typeof ticketRowSettings.claimEta.value;
        break;
      case 'confidence':
        ticketRowSettings.confidence.value = control.value;
        break;
      case null:
        break;
    }
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.selectTicketRow.selector, (event, target) => {
    if ((event.target as Element).closest('[data-action="toggle-row-up-next"]')) return;
    const row = target as HTMLElement;
    if (usesCollectionState()) {
      const pointer = event as MouseEvent;
      selectCollectionTicket(row.dataset.ticketSlug!, {
        range: pointer.shiftKey,
        toggle: pointer.metaKey || pointer.ctrlKey,
      });
      // Like the workspace grid's rail, a plain click in the TerminalTicketRail demo pushes that ticket's
      // detail onto its NavStack (HS2-FY06N4).
      if (row.closest('.terminal-ticket-rail-demo') && !pointer.shiftKey && !pointer.metaKey && !pointer.ctrlKey) {
        terminalRailDemoTicket.value = row.dataset.ticketSlug;
        recordCollectionEvent(`${row.dataset.ticketSlug} pushed onto the ticket rail`);
      }
      return;
    }
    ticketRowSettings.selected.value = !ticketRowSettings.selected.value;
    ticketRowSettings.event.value = ticketRowSettings.selected.value ? 'Ticket selected' : 'Ticket deselected';
    const selected = root.querySelector<FormControl>('[data-settings="ticket-list-row"] [name="selected"]');
    if (selected) selected.checked = ticketRowSettings.selected.value;
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.selectTicketColumn.selector, (event, target) => {
    event.stopImmediatePropagation();
    const column = (target as HTMLElement).closest<HTMLElement>('[data-component="ticket-board-column"]');
    if (!column || !usesCollectionState()) return;
    const slugs = new Set(
      [...column.querySelectorAll<HTMLElement>('[data-ticket-slug]')].map((row) => row.dataset.ticketSlug),
    );
    collectionTickets.value = collectionTickets.value.map((ticket) => ({
      ...ticket,
      selected: slugs.has(ticket.slug),
    }));
    recordCollectionEvent(`${slugs.size} tickets selected`);
  }),
);
function toggleRowUpNext(target?: Element): void {
  if (usesCollectionState()) {
    const row = target?.closest('[data-component="ticket-list-row"]') as HTMLElement | null;
    if (row) toggleCollectionTicketUpNext(row.dataset.ticketSlug!);
    return;
  }
  ticketRowSettings.upNext.value = !ticketRowSettings.upNext.value;
  ticketRowSettings.event.value = ticketRowSettings.upNext.value ? 'Added to Up Next' : 'Removed from Up Next';
  const control = root.querySelector<FormControl>('[data-settings="ticket-list-row"] [name="up-next"]');
  if (control) control.checked = ticketRowSettings.upNext.value;
}
demoListeners.add(
  delegateCapture(root, 'click', DEMO_ACTIONS.toggleRowUpNext.selector, (event) => {
    event.stopPropagation();
    toggleRowUpNext(event.target as Element);
  }),
);
demoListeners.add(
  delegateCapture(root, 'keydown', DEMO_ACTIONS.toggleRowUpNext.selector, (event) => {
    const key = (event as KeyboardEvent).key;
    if (key !== 'Enter' && key !== ' ') return;
    event.preventDefault();
    event.stopPropagation();
    toggleRowUpNext(event.target as Element);
  }),
);
demoListeners.add(
  delegate(root, 'keydown', DEMO_ACTIONS.selectTicketRow.selector, (event, target) => {
    const keyboard = event as KeyboardEvent;
    const key = keyboard.key;
    const row = target as HTMLElement;
    if (usesCollectionState() && (keyboard.metaKey || keyboard.ctrlKey) && key.toLowerCase() === 'a') {
      event.preventDefault();
      selectAllCollectionTickets();
      return;
    }
    if (usesCollectionState() && (key === 'ArrowUp' || key === 'ArrowDown')) {
      event.preventDefault();
      const scope = row.closest<HTMLElement>('[data-ticket-selection-root]')!;
      const rows = [...scope.querySelectorAll<HTMLElement>('[data-action="select-ticket-row"]')];
      const index = rows.indexOf(row);
      const next = rows[Math.max(0, Math.min(rows.length - 1, index + (key === 'ArrowDown' ? 1 : -1)))];
      next.focus();
      selectCollectionTicket(next.dataset.ticketSlug!, {
        range: keyboard.shiftKey,
      });
      return;
    }
    if (key !== 'Enter' && key !== ' ') return;
    event.preventDefault();
    if (usesCollectionState())
      selectCollectionTicket(row.dataset.ticketSlug!, {
        range: keyboard.shiftKey,
        toggle: keyboard.metaKey || keyboard.ctrlKey,
      });
    else row.click();
  }),
);
demoListeners.add(
  delegate(root, 'contextmenu', DEMO_ACTIONS.selectTicketRow.selector, (event, target) => {
    event.preventDefault();
    const pointer = event as MouseEvent;
    const row = target as HTMLElement;
    if (usesCollectionState()) {
      if (!collectionTickets.value.find((ticket) => ticket.slug === row.dataset.ticketSlug)?.selected)
        selectCollectionTicket(row.dataset.ticketSlug!);
      recordCollectionEvent(`Context menu opened for ${row.dataset.ticketSlug}`);
      contextMenu.value = {
        x: pointer.clientX,
        y: pointer.clientY,
        ticketSlug: row.dataset.ticketSlug,
      };
      return;
    }
    ticketRowSettings.selected.value = true;
    ticketRowSettings.event.value = 'Context menu opened';
    const selected = root.querySelector<FormControl>('[data-settings="ticket-list-row"] [name="selected"]');
    if (selected) selected.checked = true;
    contextMenu.value = {
      x: pointer.clientX,
      y: pointer.clientY,
      ticketSlug: row.dataset.ticketSlug,
    };
  }),
);
demoListeners.add(
  delegate(root, 'dblclick', DEMO_ACTIONS.selectTicketRow.selector, (event, target) => {
    if ((event.target as Element).closest('button, input, textarea, select, a, [contenteditable="true"]')) return;
    if (usesCollectionState()) selectCollectionTicket((target as HTMLElement).dataset.ticketSlug!);
    recordCollectionEvent(`Ticket reader opened for ${(target as HTMLElement).dataset.ticketSlug}`);
    openDemoTicketReader();
  }),
);
demoListeners.add(
  delegate(root, 'click', '[data-context-field]', (event, target) => {
    event.stopPropagation();
    const field = (target as HTMLElement).dataset.contextField as 'category' | 'priority' | 'status';
    const value = (target as HTMLElement).dataset.contextValue!;
    if (usesCollectionState() && contextMenu.value?.ticketSlug) {
      const selected = new Set(
        collectionTickets.value.filter((ticket) => ticket.selected).map((ticket) => ticket.slug),
      );
      if (!selected.size) selected.add(contextMenu.value.ticketSlug);
      collectionTickets.value = collectionTickets.value.map((ticket) => {
        if (!selected.has(ticket.slug)) return ticket;
        if (field === 'category') return { ...ticket, category: value };
        if (field === 'priority') return { ...ticket, priority: value as typeof ticket.priority };
        return { ...ticket, status: value as typeof ticket.status };
      });
      recordCollectionEvent(
        `${field} changed to ${value} for ${selected.size} ticket${selected.size === 1 ? '' : 's'}`,
      );
    }
    contextMenu.value = undefined;
  }),
);
demoListeners.add(
  delegate(root, 'click', '[data-context-action]', (_event, target) => {
    const action = (target as HTMLElement).dataset.contextAction!;
    if (usesCollectionState() && contextMenu.value?.ticketSlug) {
      const slug = contextMenu.value.ticketSlug;
      if (action === 'Toggle Up Next') toggleCollectionTicketUpNext(slug);
      if (action === 'Report not working') {
        selectDemo('not-working-dialog');
        notWorkingDemoOpen.value = true;
      }
      recordCollectionEvent(`${action} selected for ${slug}`);
      contextMenu.value = undefined;
      return;
    }
    if (action === 'Toggle Up Next') {
      ticketRowSettings.upNext.value = !ticketRowSettings.upNext.value;
      const control = root.querySelector<FormControl>('[data-settings="ticket-list-row"] [name="up-next"]');
      if (control) control.checked = ticketRowSettings.upNext.value;
    }
    ticketRowSettings.event.value = `${action} selected`;
    contextMenu.value = undefined;
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.openNotWorkingDemo.selector, () => {
    notWorkingDemoOpen.value = true;
    notWorkingDemoEvent.value = '';
    queueMicrotask(() => root.querySelector<HTMLTextAreaElement>('[name="not-working-note"]')?.focus());
  }),
);
demoListeners.add(
  delegate(root, 'input', DEMO_FIELDS.notWorkingNote.selector, (_event, target) => {
    notWorkingDemoNote.value = (target as HTMLTextAreaElement).value;
  }),
);
demoListeners.add(
  delegate(root, 'change', 'input[name="not-working-attachments"]', (_event, target) => {
    const input = target as HTMLInputElement;
    if (input.files?.length)
      notWorkingDemoFiles.value = [
        ...notWorkingDemoFiles.value,
        ...Array.from(input.files).map((file, index) => ({
          id: `demo-${Date.now()}-${index}`,
          name: file.name,
        })),
      ];
    input.value = '';
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.removeNotWorkingAttachment.selector, (_event, target) => {
    notWorkingDemoFiles.value = notWorkingDemoFiles.value.filter(
      (item) => item.id !== (target as HTMLElement).dataset.pendingAttachmentId,
    );
  }),
);
demoListeners.add(
  delegate(root, 'dragover', DEMO_MARKERS.notWorkingDropzone.selector, (event, target) => {
    event.preventDefault();
    (target as HTMLElement).dataset.dragging = 'true';
  }),
);
demoListeners.add(
  delegate(root, 'dragleave', DEMO_MARKERS.notWorkingDropzone.selector, (_event, target) => {
    delete (target as HTMLElement).dataset.dragging;
  }),
);
demoListeners.add(
  delegate(root, 'drop', DEMO_MARKERS.notWorkingDropzone.selector, (event, target) => {
    event.preventDefault();
    delete (target as HTMLElement).dataset.dragging;
    const files = (event as DragEvent).dataTransfer?.files;
    if (files?.length)
      notWorkingDemoFiles.value = [
        ...notWorkingDemoFiles.value,
        ...Array.from(files).map((file, index) => ({
          id: `drop-${Date.now()}-${index}`,
          name: file.name,
        })),
      ];
  }),
);
demoListeners.add(
  delegate(root, 'submit', DEMO_ACTIONS.submitNotWorking.selector, (event) => {
    event.preventDefault();
    notWorkingDemoEvent.value = 'Ticket returned to Not Started and added to Up Next.';
    notWorkingDemoOpen.value = false;
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.cancelNotWorking.selector, () => {
    notWorkingDemoOpen.value = false;
    notWorkingDemoEvent.value = 'Report cancelled.';
  }),
);
demoListeners.add(
  delegate(root, 'wa-request-close', DEMO_COMPONENTS.notWorkingDialog.selector, (event) => {
    event.preventDefault();
  }),
);
// CommandRunDialog demo: swap between its run-output and stop-confirmation presentations and stand in
// for the app's dismiss/stop side effects (HS2-CWWX7S).
demoListeners.add(
  delegate(root, 'change', '[data-settings="command-run-dialog"] [name="presentation"]', (_event, target) => {
    setCommandRunDialogPresentation(root, (target as FormControl).value as CommandRunDialogPresentation);
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.openCommandRunDialogDemo.selector, () => {
    showCommandRunDialogDemo(root);
  }),
);
demoListeners.add(
  delegate(root, 'click', `dialog.command-run-dialog ${COMMANDS_AND_AI_ACTIONS.dismissCommandDialog.selector}`, () => {
    dismissCommandRunDialogDemo(root);
  }),
);
demoListeners.add(
  delegate(
    root,
    'click',
    `dialog.command-run-dialog ${COMMANDS_AND_AI_ACTIONS.confirmStopCommand.selector}`,
    (_event, target) => {
      confirmStopCommandRunDemo(root, (target as HTMLElement).dataset.runId);
    },
  ),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.openGalleryDemo.selector, () => {
    setGalleryDemo(true);
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.closeAttachmentGallery.selector, () => {
    setGalleryDemo(false);
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.previousGalleryImage.selector, () => {
    shiftGalleryDemo(-1);
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.nextGalleryImage.selector, () => {
    shiftGalleryDemo(1);
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.zoomGalleryImage.selector, (_event, target) => {
    zoomGalleryDemo(target.getAttribute('data-zoom-direction') === 'out' ? 'out' : 'in');
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.toggleGalleryMarkup.selector, () => {
    galleryDemoMarkup.value = !galleryDemoMarkup.value;
    galleryDemoDrawMode.value = false;
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.toggleGalleryDraw.selector, () => {
    galleryDemoDrawMode.value = !galleryDemoDrawMode.value;
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.toggleGalleryPlayback.selector, () => {
    galleryDemoPlaying.value = !galleryDemoPlaying.value;
  }),
);
demoListeners.add(
  delegate(root, 'keydown', DEMO_COMPONENTS.attachmentGallery.selector, (event) => {
    const keyboard = event as KeyboardEvent,
      origin = event.target as Element,
      playheadControl = origin.matches('input[name="gallery-playhead"]');
    if (
      !root.querySelector('.attachment-gallery video') ||
      (!playheadControl && origin.closest('button,input,textarea,select,[contenteditable="true"]'))
    )
      return;
    const action = attachmentGalleryKeyboardAction(keyboard.key, galleryDemoPlayhead.value, 6000, keyboard.shiftKey);
    if (!action) return;
    event.preventDefault();
    if (action.kind === 'toggle-playback') galleryDemoPlaying.value = !galleryDemoPlaying.value;
    else {
      galleryDemoPlaying.value = false;
      galleryDemoPlayhead.value = action.playheadMs;
    }
  }),
);
demoListeners.add(
  delegate(root, 'input', 'input[name="gallery-playhead"]', (_event, target) => {
    galleryDemoPlayhead.value = Number((target as HTMLInputElement).value);
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.seekGalleryAnnotation.selector, (_event, target) => {
    const element = target as HTMLElement;
    galleryDemoPlayhead.value = Number(element.dataset.annotationTime ?? 0);
    if (galleryDemoMarkup.value && element.dataset.annotationId)
      galleryDemoSelectedAnnotation.value = element.dataset.annotationId;
  }),
);
demoListeners.add(
  delegate(root, 'input', 'input[name="gallery-volume"]', (_event, target) => {
    galleryDemoVolume.value = Number((target as HTMLInputElement).value);
    galleryDemoMuted.value = false;
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.toggleGalleryVolume.selector, () => {
    galleryDemoVolumeOpen.value = !galleryDemoVolumeOpen.value;
  }),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.toggleGalleryMuted.selector, () => {
    galleryDemoMuted.value = !galleryDemoMuted.value;
  }),
);
root.addEventListener('click', (event) => {
  if (galleryDemoVolumeOpen.value && !(event.target as Element).closest('.attachment-gallery__volume'))
    galleryDemoVolumeOpen.value = false;
});
demoListeners.add(
  delegateCapture(root, 'pointerdown', DEMO_MARKERS.galleryAnnotationSurface.selector, (event) => {
    if (!galleryDemoMarkup.value || (event.target as Element).closest('[data-annotation-id]')) return;
    galleryDemoSelectedAnnotation.value = undefined;
  }),
);
demoListeners.add(
  delegate(root, 'keydown', '[data-gallery-range-handle]', (event, target) => {
    const keyboard = event as KeyboardEvent;
    if (keyboard.key !== 'ArrowLeft' && keyboard.key !== 'ArrowRight') return;
    event.preventDefault();
    const endpoint = (target as HTMLElement).dataset.galleryRangeHandle;
    const annotation = galleryDemoVideoAnnotations.value[0];
    if (endpoint !== 'start' && endpoint !== 'end') return;
    setGalleryDemoAnnotationEndpoint(
      endpoint,
      (endpoint === 'start' ? annotation.start_ms : annotation.end_ms) + (keyboard.key === 'ArrowLeft' ? -100 : 100),
    );
  }),
);
let galleryDemoRangeGesture: { pointerId: number; endpoint: 'start' | 'end'; track: DOMRect } | undefined;
demoListeners.add(
  delegateCapture(root, 'pointerdown', '[data-gallery-range-handle]', (event, target) => {
    const pointer = event as PointerEvent,
      element = target as HTMLElement,
      endpoint = element.dataset.galleryRangeHandle,
      track = element.closest<HTMLElement>('.attachment-gallery__timeline-track')?.getBoundingClientRect();
    if ((endpoint !== 'start' && endpoint !== 'end') || !track) return;
    event.preventDefault();
    galleryDemoRangeGesture = { pointerId: pointer.pointerId, endpoint, track };
  }),
);
document.addEventListener('pointermove', (event) => {
  const gesture = galleryDemoRangeGesture;
  if (!gesture || event.pointerId !== gesture.pointerId) return;
  event.preventDefault();
  setGalleryDemoAnnotationEndpoint(
    gesture.endpoint,
    ((event.clientX - gesture.track.left) * 6000) / gesture.track.width,
  );
});
document.addEventListener('pointerup', (event) => {
  if (!galleryDemoRangeGesture || event.pointerId !== galleryDemoRangeGesture.pointerId) return;
  galleryDemoRangeGesture = undefined;
});
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.openAttachmentMenu.selector, (event, target) => {
    event.stopPropagation();
    const rect = target.getBoundingClientRect();
    showAttachmentDemoMenu(rect.right, rect.bottom);
  }),
);
demoListeners.add(
  delegate(
    root,
    'contextmenu',
    '[data-component="ticket-attachment-item"][data-attachment-menu-kind="item"]',
    (event) => {
      event.preventDefault();
      const pointer = event as MouseEvent;
      showAttachmentDemoMenu(pointer.clientX, pointer.clientY);
    },
  ),
);
demoListeners.add(
  delegate(root, 'dblclick', DEMO_ACTIONS.editAttachmentBatchLabel.selector, (_event, target) => {
    const input = attachmentDemoLabelEditor.begin(target);
    if (!input) return;
    queueMicrotask(() => {
      input.focus();
      input.select();
    });
  }),
);
demoListeners.add(
  delegate(root, 'keydown', DEMO_FIELDS.attachmentBatchLabel.selector, (event, target) => {
    const input = target as HTMLInputElement;
    const key = (event as KeyboardEvent).key;
    if (key !== 'Escape' && key !== 'Enter') return;
    const ids = input.closest<HTMLElement>('[data-attachment-ids]')?.dataset.attachmentIds;
    if (key === 'Escape') attachmentDemoLabelEditor.escape(input);
    input.blur();
    requestAnimationFrame(() =>
      requestAnimationFrame(() => {
        if (ids)
          root
            .querySelector<HTMLElement>(
              `[data-component="ticket-attachments"] [data-attachment-ids="${CSS.escape(ids)}"] [data-action="edit-attachment-batch-label"]`,
            )
            ?.focus();
      }),
    );
  }),
);
demoListeners.add(
  delegate(root, 'input', DEMO_FIELDS.attachmentBatchLabel.selector, (_event, target) => {
    attachmentDemoLabelEditor.input(target as HTMLInputElement);
  }),
);
demoListeners.add(
  delegateCapture(root, 'blur', DEMO_FIELDS.attachmentBatchLabel.selector, (_event, target) => {
    void attachmentDemoLabelEditor.finish(target);
  }),
);
const flushDemoLabelOnHide = () => {
  void attachmentDemoLabelEditor.flush();
};
window.addEventListener('pagehide', flushDemoLabelOnHide);
demoListeners.add(() => {
  window.removeEventListener('pagehide', flushDemoLabelOnHide);
});
let draggedDemoAttachment: string | undefined;
const clearDemoAttachmentDrag = () => {
  draggedDemoAttachment = undefined;
  const surface = root.querySelector<HTMLElement>('[data-component="ticket-attachments"]');
  if (surface) delete surface.dataset.draggingGroupAttachment;
  for (const target of root.querySelectorAll<HTMLElement>('[data-drag-over]')) delete target.dataset.dragOver;
};
demoListeners.add(
  delegate(root, 'dragstart', '[data-drag-attachment-id]', (event, target) => {
    draggedDemoAttachment = (target as HTMLElement).dataset.dragAttachmentId;
    const surface = target.closest<HTMLElement>('[data-component="ticket-attachments"]');
    if (surface) surface.dataset.draggingGroupAttachment = 'true';
    const transfer = (event as DragEvent).dataTransfer;
    if (transfer && draggedDemoAttachment) transfer.setData('application/x-hotsheet-attachment', draggedDemoAttachment);
  }),
);
demoListeners.add(delegate(root, 'dragend', '[data-drag-attachment-id]', clearDemoAttachmentDrag));
demoListeners.add(
  delegate(
    root,
    'dragover',
    '[data-attachment-group-drop-target], [data-attachment-new-group-drop-target]',
    (event, target) => {
      if (!draggedDemoAttachment) return;
      event.preventDefault();
      (target as HTMLElement).dataset.dragOver = 'true';
    },
  ),
);
demoListeners.add(
  delegate(
    root,
    'drop',
    '[data-attachment-group-drop-target], [data-attachment-new-group-drop-target]',
    (event, target) => {
      if (!draggedDemoAttachment) return;
      event.preventDefault();
      event.stopPropagation();
      const id = draggedDemoAttachment;
      const newGroup = target.matches('[data-attachment-new-group-drop-target]');
      const batch = target.closest<HTMLElement>('[data-attachment-group-drop-target]')?.dataset.attachmentBatch;
      clearDemoAttachmentDrag();
      regroupAttachmentDemo(id, newGroup ? undefined : batch);
    },
  ),
);
demoListeners.add(
  delegate(root, 'click', DEMO_ACTIONS.attachmentMenuAction.selector, (_event, target) => {
    recordCollectionEvent(`${target.textContent.trim() || 'Attachment action'} selected`);
    closeAttachmentDemoMenu();
  }),
);
addEventListener(
  'pointerdown',
  (event) => {
    if (contextMenu.value && !eventTargetsContextMenu(event)) contextMenu.value = undefined;
    if (tabContextMenu.value && !eventTargetsContextMenu(event, '.project-tab-context-menu'))
      tabContextMenu.value = undefined;
    if (
      terminalDashboardContextMenu.value &&
      !(event.target as Element).closest(
        '[data-component="terminal-context-menu"], [data-action="open-terminal-context-menu"]',
      )
    )
      terminalDashboardContextMenu.value = undefined;
    if (
      attachmentDemoMenu.value &&
      !(event.target as Element).closest(
        '[data-component="attachment-context-menu"], [data-action="open-attachment-menu"]',
      )
    )
      closeAttachmentDemoMenu();
  },
  { capture: true },
);
addEventListener('keydown', (event) => {
  if (event.key === 'Escape') contextMenu.value = undefined;
  if (event.key === 'Escape') terminalDashboardContextMenu.value = undefined;
  if (event.key === 'Escape') closeAttachmentDemoMenu();
});
