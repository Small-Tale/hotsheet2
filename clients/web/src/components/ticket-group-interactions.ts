import { delegate } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import { viewportSafePointerPosition } from '../context-menu-position';
import { TICKET_GROUP_ACTIONS, TICKET_GROUP_TARGETS } from '../interaction-attrs/ticket-groups';

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
const scopeOf = (target: Element) => target.closest<HTMLElement>(groupSelector)?.dataset.ticketGroupScope ?? 'list';

/** Wire ticket group headers, their menu, and the group name dialog under `root`. */
export function wireTicketGroups(root: HTMLElement, bindings: TicketGroupBindings): () => void {
  const lifetime = createScope();
  lifetime.add(
    delegate(root, 'click', TICKET_GROUP_ACTIONS.toggleTicketGroup.selector, (event, target) => {
      event.stopPropagation();
      const group = groupOf(target);
      if (group) bindings.toggle(group);
    }),
  );
  lifetime.add(
    delegate(root, 'contextmenu', `${groupSelector} [data-component="list-header"]`, (event, target) => {
      const group = groupOf(target);
      if (!group) return;
      event.preventDefault();
      event.stopPropagation();
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
