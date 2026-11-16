import { delegate } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import { viewportSafeContextMenuPosition, viewportSafePointerPosition } from '../context-menu-position';
import { TICKET_GROUP_ACTIONS, TICKET_GROUP_TARGETS } from '../interaction-attrs/ticket-groups';
import { closedPopupMenuRoot } from '../popup-menu-state';

/**
 * Side effects for ticket group headers, the group header menu, and the group name dialog
 * (HS2-WZ3JGZ, HS2-TX17CM, HS2-8CDFKR). Production passes the ticket groups controller; the UX
 * demo passes fixture state, so both run the same handlers.
 */
export interface TicketGroupBindings {
  /** Collapse or expand a group. */
  readonly toggle: (group: string) => void;
  /** Open the group header menu at a viewport point, for the group's container in `scope`. */
  readonly openMenu: (group: string, x: number, y: number, scope: string) => void;
  /** Select every ticket the group shows in `scope`: the list, or one board column (HS2-EYEGDQ). */
  readonly selectAll: (group: string, scope: string) => void;
  readonly rename: (group: string) => void;
  readonly ungroup: (group: string) => void;
  /** Merge every same-named duplicate group into this one (HS2-9KZSD4). */
  readonly merge: (group: string) => void;
  /** The dialog's name field changed. */
  readonly editName: (name: string) => void;
  readonly submitName: (name: string) => void;
  /** Cancel, or the dialog hid by any other route. */
  readonly dismissDialog: () => void;
}

const groupSelector = TICKET_GROUP_TARGETS.group.selector;
const groupOf = (target: Element) =>
  (target as HTMLElement).dataset.ticketGroup ?? target.closest<HTMLElement>(groupSelector)?.dataset.ticketGroup;
/** The group header menu's footprint, so a menu opened from the overflow button stays on screen. */
export const TICKET_GROUP_MENU_SIZE = { width: 232, height: 184 } as const;
const scopeOf = (target: Element) => target.closest<HTMLElement>(groupSelector)?.dataset.ticketGroupScope ?? 'list';

/** The overflow button of one group container, found again after a render may have replaced it. */
function overflowButton(root: HTMLElement, group: string, scope: string): HTMLElement | undefined {
  return [...root.querySelectorAll<HTMLElement>(TICKET_GROUP_ACTIONS.openTicketGroupMenu.selector)].find(
    (button) => groupOf(button) === group && scopeOf(button) === scope,
  );
}

/** Wire ticket group headers, their menu, and the group name dialog under `root`. */
export function wireTicketGroups(root: HTMLElement, bindings: TicketGroupBindings): () => void {
  const lifetime = createScope();
  /** The overflow button that opened the group menu, which gets focus back when it closes. */
  let opener: { group: string; scope: string } | undefined;
  lifetime.add(
    delegate(root, 'click', TICKET_GROUP_ACTIONS.toggleTicketGroup.selector, (event, target) => {
      event.stopPropagation();
      const group = groupOf(target);
      if (group) bindings.toggle(group);
    }),
  );
  lifetime.add(
    // The header's overflow button opens the same menu under itself, aligned to its trailing edge,
    // and never toggles the group (HS2-HKXSB1).
    delegate(root, 'click', TICKET_GROUP_ACTIONS.openTicketGroupMenu.selector, (event, target) => {
      event.stopPropagation();
      const group = groupOf(target);
      if (!group) return;
      const rect = target.getBoundingClientRect(),
        position = viewportSafeContextMenuPosition(
          rect.right - TICKET_GROUP_MENU_SIZE.width,
          rect.bottom,
          window.innerWidth,
          window.innerHeight,
          TICKET_GROUP_MENU_SIZE,
        );
      opener = { group, scope: scopeOf(target) };
      bindings.openMenu(group, position.x, position.y, opener.scope);
    }),
  );
  // Closing a menu the overflow button opened returns focus to that button, unless a command
  // already moved it somewhere, such as into the rename dialog (HS2-HKXSB1).
  const returnFocus = (event: Event) => {
    const menu = closedPopupMenuRoot(event);
    if (!menu?.matches(TICKET_GROUP_TARGETS.groupMenu.selector) || !opener) return;
    const { group, scope } = opener;
    opener = undefined;
    requestAnimationFrame(() => {
      const active = document.activeElement;
      if (active && active !== document.body && !menu.contains(active)) return;
      overflowButton(root, group, scope)?.focus();
    });
  };
  root.addEventListener('toggle', returnFocus, true);
  lifetime.add(() => {
    root.removeEventListener('toggle', returnFocus, true);
  });
  lifetime.add(
    delegate(root, 'contextmenu', `${groupSelector} [data-component="list-header"]`, (event, target) => {
      const group = groupOf(target);
      if (!group) return;
      event.preventDefault();
      event.stopPropagation();
      opener = undefined;
      const pointer = event as MouseEvent,
        // A keyboard-opened context menu reports no pointer position; anchor it under the header.
        rect = target.getBoundingClientRect(),
        keyboard = pointer.clientX === 0 && pointer.clientY === 0,
        position = viewportSafePointerPosition(
          keyboard ? rect.left + 16 : pointer.clientX,
          keyboard ? rect.bottom : pointer.clientY,
          window.innerWidth,
          window.innerHeight,
        );
      bindings.openMenu(group, position.x, position.y, scopeOf(target));
    }),
  );
  lifetime.add(
    delegate(root, 'click', TICKET_GROUP_ACTIONS.selectTicketGroup.selector, (_event, target) => {
      const { ticketGroup, ticketGroupScope } = (target as HTMLElement).dataset;
      if (ticketGroup) bindings.selectAll(ticketGroup, ticketGroupScope ?? 'list');
    }),
  );
  lifetime.add(
    delegate(root, 'click', TICKET_GROUP_ACTIONS.renameTicketGroup.selector, (_event, target) => {
      const group = (target as HTMLElement).dataset.ticketGroup;
      if (group) bindings.rename(group);
    }),
  );
  lifetime.add(
    delegate(root, 'click', TICKET_GROUP_ACTIONS.ungroupTicketGroup.selector, (_event, target) => {
      const group = (target as HTMLElement).dataset.ticketGroup;
      if (group) bindings.ungroup(group);
    }),
  );
  lifetime.add(
    delegate(root, 'click', TICKET_GROUP_ACTIONS.mergeTicketGroups.selector, (_event, target) => {
      const group = (target as HTMLElement).dataset.ticketGroup;
      if (group) bindings.merge(group);
    }),
  );
  lifetime.add(
    delegate(root, 'input', TICKET_GROUP_TARGETS.groupNameField.selector, (_event, target) => {
      bindings.editName((target as HTMLInputElement).value);
    }),
  );
  lifetime.add(
    delegate(root, 'submit', TICKET_GROUP_ACTIONS.submitTicketGroupName.selector, (event, target) => {
      event.preventDefault();
      bindings.submitName(
        target.querySelector<HTMLInputElement>(TICKET_GROUP_TARGETS.groupNameField.selector)?.value ?? '',
      );
    }),
  );
  lifetime.add(
    delegate(root, 'click', TICKET_GROUP_ACTIONS.cancelTicketGroupName.selector, () => {
      bindings.dismissDialog();
    }),
  );
  lifetime.add(
    delegate(root, 'kui-request-close', TICKET_GROUP_TARGETS.groupNameDialog.selector, (event, target) => {
      // Only the dialog's own close request counts.
      if (event.target !== target) return;
      bindings.dismissDialog();
    }),
  );
  return () => {
    lifetime.dispose();
  };
}
