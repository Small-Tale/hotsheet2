import {
  type ResizableRegionAxis,
  type ResizableRegionEdge,
  resizeRegionFromPointer,
} from '@kerfjs/ui/resizable-region';
import { delegate, delegateCapture, type Signal } from 'kerfjs';

import { type TicketRow as WireTicketRow } from '../api';
import {
  type AppRegionId,
  isAppRegionId,
  normalizeAppRegionSize,
  TERMINAL_DRAWER_MIN_SIZE,
  terminalDrawerDragDecision,
} from '../app-region-resize';
import { type ProjectTabBarMode } from '../components/project-tab-bar';
import { type AppTabKind } from '../components/project-tab-context-menu';
import { type RepositoryFileMenu } from '../components/repository-status-popover';
import { eventTargetsContextMenu } from '../components/ticket-row-context-menu';
import { type WorkspaceViewMode } from '../components/workspace-header';
import { matchesShortcut, type ShortcutChord } from '../keyboard-shortcuts';
import {
  closeMobileOverlay,
  MOBILE_OVERLAYS_CLOSED,
  type MobileOverlayState,
  openMobileOverlay,
  toggleMobileInspector,
  toggleMobileSidebar,
} from '../mobile-layout';
import { cycleTabId } from '../tab-cycle';
import { TERMINAL_DRAWER_RESIZE_END_EVENT } from '../terminal-viewport';
import { type BulkTicketAction } from '../ticket-bulk-operations';
import { ticketClipboardAction } from '../ticket-clipboard-shortcuts';
import { type ClipboardTicket, type TicketHistory } from '../ticket-operations';
import { data } from './dom';
import { type AttachmentMenu, type Project } from './types';

/** Live application bindings used by this handler group. */
export interface ShellAndGlobalInteractionsDependencies {
  readonly viewportMobile: Signal<boolean>;
  readonly mobileOverlay: Signal<MobileOverlayState>;
  readonly setInspectorVisible: (visible: boolean) => void;
  readonly setSidebarVisible: (visible: boolean) => void;
  readonly sidebarVisible: Signal<boolean>;
  appRegionResizeDrag:
    | {
        id: AppRegionId;
        axis: ResizableRegionAxis;
        edge: ResizableRegionEdge;
        startPoint: number;
        startSize: number;
        pendingSize: number;
        collapseRequested?: boolean;
        region: HTMLElement;
        handle: HTMLElement;
        frame?: number;
      }
    | undefined;
  readonly appRegionSize: (id: AppRegionId) => number;
  readonly setTerminalDrawerVisible: (visible: boolean, refresh?: boolean) => void;
  readonly setAppRegionSize: (id: AppRegionId, size: number) => void;
  readonly terminalDrawerMax: Signal<number>;
  readonly syncTerminalDrawerMaximum: () => void;
  readonly updateTerminalDrawerBounds: (target: HTMLElement, rect?: Pick<DOMRectReadOnly, 'width' | 'height'>) => void;
  draggedTickets: { slugs: string[]; source: Project } | undefined;
  readonly project: () => Project | undefined;
  readonly selectedTicketSlugs: Signal<string[]>;
  readonly tickets: Signal<WireTicketRow[]>;
  readonly history: (projectId?: string) => TicketHistory;
  readonly executeBulkTicketAction: (action: BulkTicketAction, targetSlugs?: string[]) => Promise<boolean>;
  readonly copyDraggedTickets: (destination: Project, drag: { slugs: string[]; source: Project }) => Promise<void>;
  readonly projects: Signal<Project[]>;
  readonly capturingShortcutId: Signal<string | undefined>;
  readonly keyboardShortcutOverrides: Signal<Record<string, ShortcutChord>>;
  readonly isEditableEvent: (event: Event) => boolean;
  readonly appleShortcutPlatform: boolean;
  readonly searchOpen: Signal<boolean>;
  readonly focusWorkspaceSearch: (offset?: number) => void;
  readonly inspectorVisible: Signal<boolean>;
  readonly terminalDrawerVisible: Signal<boolean>;
  readonly switchWorkspaceView: (mode: WorkspaceViewMode) => void;
  readonly setShellMode: (mode: ProjectTabBarMode) => void;
  readonly shellMode: Signal<ProjectTabBarMode>;
  readonly statsProjectId: Signal<string | undefined>;
  readonly selectedProjectId: Signal<string>;
  readonly selectProjectTab: (next: string) => void;
  readonly currentDrawerTabIds: (projectId: string) => string[];
  readonly terminalDrawerSelected: Signal<string>;
  readonly selectDrawerItem: (id: string) => void;
  readonly openTicketComposer: (trigger?: HTMLElement) => void;
  readonly ticketWorkAreaFocused: () => boolean;
  readonly ordinaryTextSelected: () => boolean;
  clipboard: { tickets: ClipboardTicket[]; cut: boolean; source: Project } | undefined;
  readonly copySelection: (cut: boolean) => void;
  readonly pasteSelection: () => Promise<void>;
  readonly ticketContextMenu: Signal<{ x: number; y: number; ticketSlug: string; hideUpNext?: boolean } | undefined>;
  readonly appTabContextMenu: Signal<
    { x: number; y: number; kind: AppTabKind; id: string; direction: 'left' | 'right' } | undefined
  >;
  readonly terminalContextMenu: Signal<{ key: string; x: number; y: number } | undefined>;
  readonly terminalVisibilityContextMenu: Signal<{ id: string; x: number; y: number } | undefined>;
  readonly repositoryFileMenu: Signal<RepositoryFileMenu | undefined>;
  readonly attachmentMenu: Signal<AttachmentMenu | undefined>;
  commandLongPressFired: boolean;
  commandLongPressTimer: number | undefined;
  readonly openCommandHistory: (commandId: string) => Promise<void>;
  readonly attachmentGalleryUrl: Signal<string | undefined>;
  readonly shiftGallery: (delta: number) => void;
  readonly resetAttachmentGallery: (url?: string) => void;
  readonly magnifiedTerminalKey: Signal<string | undefined>;
  readonly completePointerDetailsFinish: () => void;
  readonly schedulePointerDetailsFinish: () => void;
}

/** Register this group only when the application wiring owner invokes it. */
export function wireShellAndGlobalInteractions(dependencies: ShellAndGlobalInteractionsDependencies) {
  const {
    viewportMobile,
    mobileOverlay,
    setInspectorVisible,
    setSidebarVisible,
    sidebarVisible,
    appRegionSize,
    setTerminalDrawerVisible,
    setAppRegionSize,
    terminalDrawerMax,
    syncTerminalDrawerMaximum,
    updateTerminalDrawerBounds,
    project,
    selectedTicketSlugs,
    tickets,
    history,
    executeBulkTicketAction,
    copyDraggedTickets,
    projects,
    capturingShortcutId,
    keyboardShortcutOverrides,
    isEditableEvent,
    appleShortcutPlatform,
    searchOpen,
    focusWorkspaceSearch,
    inspectorVisible,
    terminalDrawerVisible,
    switchWorkspaceView,
    setShellMode,
    shellMode,
    statsProjectId,
    selectedProjectId,
    selectProjectTab,
    currentDrawerTabIds,
    terminalDrawerSelected,
    selectDrawerItem,
    openTicketComposer,
    ticketWorkAreaFocused,
    ordinaryTextSelected,
    copySelection,
    pasteSelection,
    ticketContextMenu,
    appTabContextMenu,
    terminalContextMenu,
    terminalVisibilityContextMenu,
    repositoryFileMenu,
    attachmentMenu,
    openCommandHistory,
    attachmentGalleryUrl,
    shiftGallery,
    resetAttachmentGallery,
    magnifiedTerminalKey,
    completePointerDetailsFinish,
    schedulePointerDetailsFinish,
  } = dependencies;
  delegate(document.body, 'click', '[data-action="close-ticket-inspector"]', () => {
    if (viewportMobile.value) mobileOverlay.value = closeMobileOverlay(mobileOverlay.value, 'inspector');
    else setInspectorVisible(false);
  });
  delegate(document.body, 'click', '[data-action="open-ticket-inspector"]', () => {
    if (viewportMobile.value) mobileOverlay.value = openMobileOverlay('inspector');
    else setInspectorVisible(true);
  });
  delegate(document.body, 'click', '[data-action="toggle-project-sidebar"]', () => {
    if (viewportMobile.value) mobileOverlay.value = toggleMobileSidebar(mobileOverlay.value);
    else setSidebarVisible(!sidebarVisible.value);
  });
  delegate(document.body, 'click', '[data-action="dismiss-mobile-overlays"]', () => {
    mobileOverlay.value = MOBILE_OVERLAYS_CLOSED;
  });
  delegate(document.body, 'pointerdown', '[data-kui-resize-handle]', (event, target) => {
    const handle = target as HTMLElement,
      region = handle.closest<HTMLElement>('[data-component="resizable-region"]'),
      id = handle.dataset.regionId;
    if (!region || !isAppRegionId(id) || region.dataset.collapsed === 'true') return;
    event.preventDefault();
    const axis = (region.dataset.axis ?? 'horizontal') as ResizableRegionAxis;
    dependencies.appRegionResizeDrag = {
      id,
      axis,
      edge: (region.dataset.edge ?? 'end') as ResizableRegionEdge,
      startPoint: axis === 'horizontal' ? (event as PointerEvent).clientX : (event as PointerEvent).clientY,
      startSize: appRegionSize(id),
      pendingSize: appRegionSize(id),
      region,
      handle,
    };
    region.dataset.resizing = 'true';
    document.body.dataset.resizingRegion = axis;
  });
  delegate(document.body, 'keydown', '[data-kui-resize-handle]', (event, target) => {
    const keyboard = event as KeyboardEvent,
      handle = target as HTMLElement,
      region = handle.closest<HTMLElement>('[data-component="resizable-region"]'),
      id = handle.dataset.regionId;
    if (!region || !isAppRegionId(id)) return;
    const axis = (region.dataset.axis ?? 'horizontal') as ResizableRegionAxis;
    if (
      (axis === 'horizontal' && !['ArrowLeft', 'ArrowRight'].includes(keyboard.key)) ||
      (axis === 'vertical' && !['ArrowUp', 'ArrowDown'].includes(keyboard.key))
    )
      return;
    event.preventDefault();
    const direction = ['ArrowRight', 'ArrowDown'].includes(keyboard.key) ? 1 : -1,
      edge = (region.dataset.edge ?? 'end') as ResizableRegionEdge,
      raw = resizeRegionFromPointer(appRegionSize(id), direction * 16, edge);
    if (
      id === 'app-terminal-drawer' &&
      appRegionSize(id) <= TERMINAL_DRAWER_MIN_SIZE &&
      raw < TERMINAL_DRAWER_MIN_SIZE
    ) {
      setTerminalDrawerVisible(false);
      return;
    }
    setAppRegionSize(id, raw);
  });
  window.addEventListener('pointermove', (event) => {
    const drag = dependencies.appRegionResizeDrag;
    if (!drag) return;
    const point = drag.axis === 'horizontal' ? event.clientX : event.clientY,
      raw = resizeRegionFromPointer(drag.startSize, point - drag.startPoint, drag.edge);
    if (drag.id === 'app-terminal-drawer') {
      const decision = terminalDrawerDragDecision(raw, terminalDrawerMax.value);
      drag.pendingSize = decision.size;
      drag.collapseRequested = decision.collapse;
    } else drag.pendingSize = normalizeAppRegionSize(drag.id, raw);
    if (drag.frame !== undefined) return;
    drag.frame = requestAnimationFrame(() => {
      drag.frame = undefined;
      drag.region.style.setProperty('--kui-resizable-region-size', `${drag.pendingSize}px`);
      drag.region.style.setProperty('--kui-resizable-region-expanded-size', `${drag.pendingSize}px`);
      drag.handle.setAttribute('aria-valuenow', String(drag.pendingSize));
    });
  });
  window.addEventListener('resize', syncTerminalDrawerMaximum);
  function finishAppRegionResize() {
    const drag = dependencies.appRegionResizeDrag;
    if (!drag) return;
    if (drag.frame !== undefined) cancelAnimationFrame(drag.frame);
    dependencies.appRegionResizeDrag = undefined;
    delete drag.region.dataset.resizing;
    delete document.body.dataset.resizingRegion;
    setAppRegionSize(drag.id, drag.pendingSize);
    if (drag.id === 'app-terminal-drawer' && drag.collapseRequested) {
      setTerminalDrawerVisible(false);
      return;
    }
    if (drag.id === 'app-terminal-drawer')
      requestAnimationFrame(() => {
        const target = document.querySelector<HTMLElement>(
          '[data-terminal-drawer-measure="true"] .terminal-drawer__content',
        );
        if (target) updateTerminalDrawerBounds(target);
        window.dispatchEvent(new CustomEvent(TERMINAL_DRAWER_RESIZE_END_EVENT));
      });
  }
  window.addEventListener('pointerup', finishAppRegionResize);
  window.addEventListener('pointercancel', finishAppRegionResize);
  function clearTicketDrag() {
    dependencies.draggedTickets = undefined;
    document
      .querySelectorAll<HTMLElement>('[data-dragging-ticket="true"]')
      .forEach((target) => delete target.dataset.draggingTicket);
  }
  delegate(document.body, 'dragstart', '[data-action="select-ticket-row"]', (event, target) => {
    const source = project(),
      slug = data(target).ticketSlug;
    if (!source || !slug) return;
    const slugs = selectedTicketSlugs.value.includes(slug) ? [...selectedTicketSlugs.value] : [slug];
    dependencies.draggedTickets = { slugs, source };
    const transfer = (event as DragEvent).dataTransfer;
    if (transfer) {
      transfer.effectAllowed = 'copyMove';
      transfer.setData('application/x-hotsheet-tickets', slugs.join(','));
    }
  });
  delegate(
    document.body,
    'dragover',
    '[data-ticket-drop-status], [data-ticket-drop-action], [data-ticket-drop-project]',
    (event, target) => {
      const drag = dependencies.draggedTickets;
      if (!drag) return;
      const destinationProject = data(target).ticketDropProject;
      if (destinationProject === drag.source.id) return;
      event.preventDefault();
      (target as HTMLElement).dataset.draggingTicket = 'true';
      if ((event as DragEvent).dataTransfer)
        (event as DragEvent).dataTransfer!.dropEffect =
          destinationProject || data(target).ticketDropAction ? 'copy' : 'move';
    },
  );
  delegate(
    document.body,
    'dragleave',
    '[data-ticket-drop-status], [data-ticket-drop-action], [data-ticket-drop-project]',
    (_event, target) => {
      delete (target as HTMLElement).dataset.draggingTicket;
    },
  );
  delegate(document.body, 'drop', '[data-ticket-drop-status]', (event, target) => {
    const drag = dependencies.draggedTickets;
    if (!drag || project()?.id !== drag.source.id) return;
    event.preventDefault();
    event.stopPropagation();
    const nextStatus = data(target).ticketDropStatus,
      itemId = data(target).itemId,
      rows = tickets.value.filter((ticket) => drag.slugs.includes(ticket.slug)),
      eligible = rows
        .filter((ticket) =>
          nextStatus === 'not_started' && itemId === 'all'
            ? ['backlog', 'archive', 'deleted', 'moved'].includes(ticket.status ?? '')
            : ticket.status !== nextStatus,
        )
        .map((ticket) => ticket.slug);
    clearTicketDrag();
    if (eligible.length === 1) void history().execute(eligible[0], { status: nextStatus });
    else if (eligible.length > 1)
      void executeBulkTicketAction({ kind: 'field', field: 'status', value: nextStatus! }, eligible);
  });
  delegate(document.body, 'drop', '[data-ticket-drop-action="duplicate"]', (event) => {
    const drag = dependencies.draggedTickets,
      destination = project();
    if (!drag || !destination) return;
    event.preventDefault();
    event.stopPropagation();
    clearTicketDrag();
    void copyDraggedTickets(destination, drag);
  });
  delegate(document.body, 'drop', '[data-ticket-drop-project]', (event, target) => {
    const drag = dependencies.draggedTickets,
      destination = projects.value.find((item) => item.id === data(target).ticketDropProject);
    if (!drag || !destination || destination.id === drag.source.id) return;
    event.preventDefault();
    event.stopPropagation();
    clearTicketDrag();
    void copyDraggedTickets(destination, drag);
  });
  delegate(document.body, 'dragend', '[data-action="select-ticket-row"]', () => {
    clearTicketDrag();
  });
  document.addEventListener(
    'pointerdown',
    (event) => {
      const area = document.querySelector<HTMLElement>('.app-shell__work-area'),
        active = document.activeElement;
      if (
        area &&
        active instanceof HTMLElement &&
        (active === area || area.contains(active)) &&
        !event.composedPath().includes(area)
      )
        active.blur();
    },
    { capture: true },
  );
  document.addEventListener(
    'keydown',
    (event) => {
      if (capturingShortcutId.value !== undefined) return;
      // While a modal dialog is open these app-level shortcuts (search focus, ticket undo/redo/clipboard)
      // would reach the workspace behind the modal — suppress them so e.g. Cmd-K cannot focus the background
      // search from inside a dialog (HS2-FW4PYZ). The modal keeps its own text-field editing and shortcuts.
      if (document.querySelector('wa-dialog[open], dialog:modal')) return;
      const overrides = keyboardShortcutOverrides.value,
        editable = isEditableEvent(event);
      if (matchesShortcut('open-search', event, overrides, appleShortcutPlatform)) {
        event.preventDefault();
        searchOpen.value = true;
        focusWorkspaceSearch();
        return;
      }
      if (!editable && matchesShortcut('redo', event, overrides, appleShortcutPlatform)) {
        event.preventDefault();
        void history().redo();
        return;
      }
      if (!editable && matchesShortcut('undo', event, overrides, appleShortcutPlatform)) {
        event.preventDefault();
        void history().undo();
        return;
      }
      if (matchesShortcut('toggle-left-sidebar', event, overrides, appleShortcutPlatform)) {
        event.preventDefault();
        if (viewportMobile.value) mobileOverlay.value = toggleMobileSidebar(mobileOverlay.value);
        else setSidebarVisible(!sidebarVisible.value);
        return;
      }
      if (matchesShortcut('toggle-right-sidebar', event, overrides, appleShortcutPlatform)) {
        event.preventDefault();
        if (viewportMobile.value) mobileOverlay.value = toggleMobileInspector(mobileOverlay.value);
        else setInspectorVisible(!inspectorVisible.value);
        return;
      }
      if (matchesShortcut('toggle-bottom-drawer', event, overrides, appleShortcutPlatform)) {
        event.preventDefault();
        setTerminalDrawerVisible(!terminalDrawerVisible.value);
        return;
      }
      if (matchesShortcut('view-list', event, overrides, appleShortcutPlatform)) {
        event.preventDefault();
        switchWorkspaceView('list');
        return;
      }
      if (matchesShortcut('view-board', event, overrides, appleShortcutPlatform)) {
        event.preventDefault();
        switchWorkspaceView('board');
        return;
      }
      if (matchesShortcut('view-notifications', event, overrides, appleShortcutPlatform)) {
        event.preventDefault();
        switchWorkspaceView('notifications');
        return;
      }
      if (matchesShortcut('view-settings', event, overrides, appleShortcutPlatform)) {
        event.preventDefault();
        switchWorkspaceView('settings');
        return;
      }
      if (matchesShortcut('view-workspace-grid', event, overrides, appleShortcutPlatform)) {
        event.preventDefault();
        setShellMode(shellMode.value === 'terminals' ? 'project' : 'terminals');
        return;
      }
      if (matchesShortcut('view-all-stats', event, overrides, appleShortcutPlatform)) {
        event.preventDefault();
        if (shellMode.value === 'stats' && statsProjectId.value === undefined) setShellMode('project');
        else {
          statsProjectId.value = undefined;
          setShellMode('stats');
        }
        return;
      }
      if (
        matchesShortcut('project-tab-next', event, overrides, appleShortcutPlatform) ||
        matchesShortcut('project-tab-previous', event, overrides, appleShortcutPlatform)
      ) {
        event.preventDefault();
        const next = cycleTabId(
          projects.value.map((item) => item.id),
          selectedProjectId.value,
          matchesShortcut('project-tab-next', event, overrides, appleShortcutPlatform) ? 1 : -1,
        );
        if (next) selectProjectTab(next);
        return;
      }
      if (
        matchesShortcut('drawer-tab-next', event, overrides, appleShortcutPlatform) ||
        matchesShortcut('drawer-tab-previous', event, overrides, appleShortcutPlatform)
      ) {
        event.preventDefault();
        const drawerProject = project();
        if (drawerProject) {
          const next = cycleTabId(
            ['grid', ...currentDrawerTabIds(drawerProject.id)],
            terminalDrawerSelected.value,
            matchesShortcut('drawer-tab-next', event, overrides, appleShortcutPlatform) ? 1 : -1,
          );
          if (next) {
            setTerminalDrawerVisible(true);
            selectDrawerItem(next);
          }
        }
        return;
      }
      if (!editable && matchesShortcut('new-ticket', event, overrides, appleShortcutPlatform)) {
        event.preventDefault();
        if (project()) {
          setShellMode('project');
          openTicketComposer();
        }
        return;
      }
      const clipboardMatch = matchesShortcut('copy-tickets', event, overrides, appleShortcutPlatform)
        ? 'copy'
        : matchesShortcut('cut-tickets', event, overrides, appleShortcutPlatform)
          ? 'cut'
          : matchesShortcut('paste-tickets', event, overrides, appleShortcutPlatform)
            ? 'paste'
            : undefined;
      const action = ticketClipboardAction({
        action: clipboardMatch,
        ticketWorkAreaFocused: ticketWorkAreaFocused(),
        editable: editable || Boolean(document.querySelector('wa-dialog[open]')),
        textSelected: ordinaryTextSelected(),
        hasTicketSelection: selectedTicketSlugs.value.length > 0,
        hasTicketClipboard: Boolean(dependencies.clipboard?.tickets.length),
      });
      if (!action) return;
      event.preventDefault();
      if (action === 'copy' || action === 'cut') copySelection(action === 'cut');
      else void pasteSelection();
    },
    { capture: true },
  );
  document.addEventListener(
    'pointerdown',
    (event) => {
      if (ticketContextMenu.value && !eventTargetsContextMenu(event)) ticketContextMenu.value = undefined;
      if (appTabContextMenu.value && !(event.target as Element).closest('.app-tab-context-menu'))
        appTabContextMenu.value = undefined;
      if (terminalContextMenu.value && !(event.target as Element).closest('[data-component="terminal-context-menu"]'))
        terminalContextMenu.value = undefined;
      if (
        terminalVisibilityContextMenu.value &&
        !(event.target as Element).closest('.terminal-visibility-dialog__context-menu')
      )
        terminalVisibilityContextMenu.value = undefined;
      if (
        repositoryFileMenu.value &&
        !(event.target as Element).closest('[data-component="repository-file-context-menu"]')
      )
        repositoryFileMenu.value = undefined;
      if (attachmentMenu.value && !(event.target as Element).closest('[data-component="attachment-context-menu"]'))
        attachmentMenu.value = undefined;
    },
    { capture: true },
  );
  document.addEventListener('pointerdown', (event) => {
    const target = (event.target as Element).closest<HTMLElement>('[data-action="run-command"]');
    if (!target) return;
    dependencies.commandLongPressFired = false;
    if (dependencies.commandLongPressTimer !== undefined) window.clearTimeout(dependencies.commandLongPressTimer);
    dependencies.commandLongPressTimer = window.setTimeout(() => {
      dependencies.commandLongPressTimer = undefined;
      dependencies.commandLongPressFired = true;
      void openCommandHistory(target.dataset.itemId!);
    }, 550);
  });
  for (const eventName of ['pointerup', 'pointercancel'] as const)
    document.addEventListener(eventName, () => {
      if (dependencies.commandLongPressTimer !== undefined) {
        window.clearTimeout(dependencies.commandLongPressTimer);
        dependencies.commandLongPressTimer = undefined;
      }
    });
  document.addEventListener('keydown', (event) => {
    if (
      !event.defaultPrevented &&
      attachmentGalleryUrl.value &&
      !document.querySelector('.attachment-gallery video') &&
      ['ArrowLeft', 'ArrowRight'].includes(event.key)
    ) {
      event.preventDefault();
      shiftGallery(event.key === 'ArrowLeft' ? -1 : 1);
      return;
    }
    if (event.key === 'Escape') {
      const galleryOpen = Boolean(attachmentGalleryUrl.value);
      ticketContextMenu.value = undefined;
      appTabContextMenu.value = undefined;
      terminalContextMenu.value = undefined;
      repositoryFileMenu.value = undefined;
      attachmentMenu.value = undefined;
      resetAttachmentGallery();
      magnifiedTerminalKey.value = undefined;
      if (galleryOpen) {
        event.preventDefault();
        event.stopImmediatePropagation();
      }
    }
  });
  delegate(document.body, 'click', '*', completePointerDetailsFinish);
  delegateCapture(document.body, 'pointerup', '*', schedulePointerDetailsFinish);
  delegateCapture(document.body, 'pointercancel', '*', schedulePointerDetailsFinish);
}
