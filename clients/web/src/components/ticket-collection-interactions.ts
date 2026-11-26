import { batch, delegate, delegateCapture } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import { viewportSafePointerPosition } from '../context-menu-position';
import { TICKET_SELECTION_ACTIONS } from '../interaction-attrs/ticket-selection';
import { insideOrchestrationView } from './orchestration-timeline-interactions';
import {
  adjacentTicketSlug,
  collectionTicketSlugs,
  decodeHiddenTicketSlugs,
  type DrawnTicketEntry,
  HIDDEN_TICKET_SLUGS_ATTRIBUTE,
  type TicketSelectionIntent,
} from './ticket-selection';

/** The ticket a row context menu was opened for, and where. */
export interface TicketCollectionMenu {
  readonly x: number;
  readonly y: number;
  readonly ticketSlug: string;
}
/** A field a ticket context menu can set on its targets. */
export type TicketContextField = 'category' | 'priority' | 'status';

/**
 * State and side effects for TicketList/TicketBoard row interactions (HS2-RWY78R). Production
 * passes the workspace selection and bulk actions; the UX demo passes fixture tickets, so both run
 * the same selection, keyboard, column, reader, and context-menu handlers.
 */
export interface TicketCollectionBindings {
  /** The currently selected ticket slugs. */
  readonly selectedSlugs: () => readonly string[];
  /** The slug order a row's selection range and arrow keys walk. */
  readonly selectionOrder: (row: HTMLElement) => string[];
  readonly select: (
    slug: string,
    intent: TicketSelectionIntent,
    ordered: string[],
    row: HTMLElement,
    event: MouseEvent | KeyboardEvent,
  ) => void;
  /** Whether a row keydown is the select-all shortcut. */
  readonly isSelectAll: (event: KeyboardEvent) => boolean;
  readonly selectAll: (row: HTMLElement) => void;
  /**
   * Select every ticket in one board column, including the members of its collapsed groups
   * (HS2-3SJ5G8). `reachable` holds the drawn rows among them.
   */
  readonly selectColumn: (slugs: string[], reachable: string[]) => void;
  readonly openReader: (slug: string, ordered: string[], row: HTMLElement) => void;
  readonly contextMenu: () => TicketCollectionMenu | undefined;
  readonly setContextMenu: (menu: TicketCollectionMenu | undefined) => void;
  /** A context-menu field choice for its target tickets (the selection, or the menu's ticket). */
  readonly onContextField: (field: TicketContextField, value: string, slugs: string[]) => void;
  /** An Add to Group choice for its target tickets; an empty name asks for a new group (HS2-TX17CM). */
  readonly onContextGroup?: (group: string, slugs: string[]) => void;
  /** A context-menu action for its target tickets, plus the ticket the menu was opened on. */
  readonly onContextAction: (action: string, slugs: string[], menuSlug: string) => void;
}

const rowSelector = TICKET_SELECTION_ACTIONS.selectTicketRow.selector;
const slugOf = (element: Element) => (element as HTMLElement).dataset.ticketSlug!;

/** The tickets a Select All takes from a drawn collection, and the drawn rows among them. */
export interface CollectionSelectAll {
  /** Every ticket in drawn order, with each collapsed group's members where the group sits. */
  readonly slugs: string[];
  /** The drawn rows a person can reach; a range selection anchors on one of these. */
  readonly reachable: string[];
}

/**
 * Read every ticket a board column or board holds, collapsed groups included (HS2-3SJ5G8): a
 * collapsed group draws no rows, so it carries its members' slugs for Select All to find.
 */
export function collectionSelectAll(container: ParentNode): CollectionSelectAll {
  const entries: DrawnTicketEntry[] = [],
    reachable: string[] = [];
  for (const element of container.querySelectorAll(`${rowSelector}, [${HIDDEN_TICKET_SLUGS_ATTRIBUTE}]`)) {
    if (element.matches(rowSelector)) {
      const slug = slugOf(element);
      entries.push({ slug });
      reachable.push(slug);
    } else
      entries.push({ hiddenSlugs: decodeHiddenTicketSlugs(element.getAttribute(HIDDEN_TICKET_SLUGS_ATTRIBUTE) ?? '') });
  }
  return { slugs: collectionTicketSlugs(entries), reachable };
}

/** Wire ticket row selection, keyboard navigation, column selection, reader, and context menu under `root`. */
export function wireTicketCollection(root: HTMLElement, bindings: TicketCollectionBindings): () => void {
  const lifetime = createScope();
  const menuTargets = (menu: TicketCollectionMenu) => {
    const selected = bindings.selectedSlugs();
    return selected.length ? [...selected] : [menu.ticketSlug];
  };
  lifetime.add(
    delegate(root, 'click', rowSelector, (event, target) => {
      if ((event.target as Element).closest('[data-action="toggle-row-up-next"]')) return;
      // HS2-J89JC7: Orchestration view cards are other projects' tickets; that view selects them.
      if (insideOrchestrationView(target)) return;
      const pointer = event as MouseEvent,
        row = target as HTMLElement;
      bindings.select(
        slugOf(row),
        { range: pointer.shiftKey, toggle: pointer.metaKey || pointer.ctrlKey },
        bindings.selectionOrder(row),
        row,
        pointer,
      );
    }),
  );
  lifetime.add(
    delegate(root, 'click', TICKET_SELECTION_ACTIONS.selectTicketColumn.selector, (event, target) => {
      event.stopImmediatePropagation();
      if (insideOrchestrationView(target)) return;
      const column = target.closest<HTMLElement>('[data-component="ticket-board-column"]');
      if (!column?.dataset.columnId) return;
      const { slugs, reachable } = collectionSelectAll(column);
      bindings.selectColumn(slugs, reachable);
    }),
  );
  lifetime.add(
    delegate(root, 'dblclick', rowSelector, (event, target) => {
      if (insideOrchestrationView(target)) return;
      if ((event.target as Element).closest('button, input, textarea, select, a, [contenteditable="true"]')) return;
      const row = target as HTMLElement;
      bindings.openReader(slugOf(row), bindings.selectionOrder(row), row);
    }),
  );
  lifetime.add(
    delegate(root, 'contextmenu', rowSelector, (event, target) => {
      if (target.closest('[data-worker-view]') || insideOrchestrationView(target)) return;
      event.preventDefault();
      const pointer = event as MouseEvent,
        row = target as HTMLElement,
        slug = slugOf(row);
      if (!bindings.selectedSlugs().includes(slug))
        bindings.select(slug, {}, bindings.selectionOrder(row), row, pointer);
      bindings.setContextMenu({
        ...viewportSafePointerPosition(pointer.clientX, pointer.clientY, window.innerWidth, window.innerHeight),
        ticketSlug: slug,
      });
    }),
  );
  lifetime.add(
    delegate(root, 'click', '[data-context-field]', (event, target) => {
      event.stopPropagation();
      const menu = bindings.contextMenu(),
        { contextField, contextValue } = (target as HTMLElement).dataset;
      if (!menu || !contextField || !contextValue) return;
      const slugs = menuTargets(menu);
      batch(() => {
        bindings.setContextMenu(undefined);
        bindings.onContextField(contextField as TicketContextField, contextValue, slugs);
      });
    }),
  );
  lifetime.add(
    delegate(root, 'click', '[data-context-group]', (event, target) => {
      event.stopPropagation();
      const menu = bindings.contextMenu(),
        group = (target as HTMLElement).dataset.contextGroup;
      if (!menu || group === undefined) return;
      const slugs = menuTargets(menu);
      batch(() => {
        bindings.setContextMenu(undefined);
        bindings.onContextGroup?.(group, slugs);
      });
    }),
  );
  lifetime.add(
    delegate(root, 'click', '[data-context-action]', (_event, target) => {
      const menu = bindings.contextMenu();
      if (!menu) return;
      const slugs = menuTargets(menu);
      bindings.setContextMenu(undefined);
      bindings.onContextAction((target as HTMLElement).dataset.contextAction!, slugs, menu.ticketSlug);
    }),
  );
  lifetime.add(
    delegate(root, 'keydown', rowSelector, (event, target) => {
      if (insideOrchestrationView(target)) return;
      const keyboard = event as KeyboardEvent,
        row = target as HTMLElement,
        slug = slugOf(row);
      if (bindings.isSelectAll(keyboard)) {
        event.preventDefault();
        bindings.selectAll(row);
        return;
      }
      if (keyboard.key === 'ArrowUp' || keyboard.key === 'ArrowDown') {
        event.preventDefault();
        const ordered = bindings.selectionOrder(row),
          next = adjacentTicketSlug(ordered, slug, keyboard.key === 'ArrowDown' ? 1 : -1);
        if (!next) return;
        const nextRow = root.querySelector<HTMLElement>(`${rowSelector}[data-ticket-slug="${CSS.escape(next)}"]`);
        nextRow?.focus();
        bindings.select(next, { range: keyboard.shiftKey }, ordered, nextRow ?? row, keyboard);
        return;
      }
      if (keyboard.key === 'Enter' || keyboard.key === ' ') {
        event.preventDefault();
        bindings.select(
          slug,
          { range: keyboard.shiftKey, toggle: keyboard.metaKey || keyboard.ctrlKey },
          bindings.selectionOrder(row),
          row,
          keyboard,
        );
      }
    }),
  );
  return () => {
    lifetime.dispose();
  };
}

/**
 * Wire the TicketRow Up Next star under `root` (HS2-NRW5ZR). The handlers run in the capture phase and
 * stop propagation so the row's own click and Enter/Space selection never see the star, which keeps the
 * star keyboard-operable inside a selectable row. `toggle` receives the star's row; a disabled star
 * (HS2-Y32T8A) is still kept from selecting its row but never toggles.
 */
export function wireTicketRowUpNextToggle(root: HTMLElement, toggle: (row: HTMLElement) => void): () => void {
  const lifetime = createScope(),
    selector = '[data-action="toggle-row-up-next"]',
    run = (target: Element) => {
      // A provider without Up Next disables the star (HS2-Y32T8A); a disabled star never toggles.
      if ((target as HTMLButtonElement).disabled) return;
      const row = target.closest<HTMLElement>('[data-ticket-slug]');
      if (row) toggle(row);
    };
  lifetime.add(
    delegateCapture(root, 'click', selector, (event, target) => {
      event.stopPropagation();
      run(target);
    }),
  );
  lifetime.add(
    delegateCapture(root, 'keydown', selector, (event, target) => {
      const key = (event as KeyboardEvent).key;
      if (key !== 'Enter' && key !== ' ') return;
      event.preventDefault();
      event.stopPropagation();
      run(target);
    }),
  );
  return () => {
    lifetime.dispose();
  };
}
