import { delegate } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import { TICKET_GROUP_SETTINGS_ACTIONS, TICKET_GROUP_TARGETS } from '../interaction-attrs/ticket-groups';

/** What Project Settings › Ticket groups does for each action (HS2-GXKX1Q). */
export interface TicketGroupSettingsBindings {
  newGroup(): void;
  viewTickets(group: string): void;
  rename(group: string): void;
  merge(group: string): void;
  remove(group: string): void;
  chooseMergeDestination(group: string): void;
  submitMerge(): void;
  cancelMerge(): void;
}

/** The group an action targets: a menu item's `data-group-id`, or the row's own. */
function groupOf(target: Element): string | undefined {
  return target.closest<HTMLElement>('[data-group-id]')?.dataset.groupId;
}

/**
 * Wire Project Settings › Ticket groups (HS2-GXKX1Q): the toolbar's New group, each row's View tickets
 * and "…" menu commands, and the Merge into… dialog. Shared by the app and its UX demo; returns a
 * disposer.
 */
export function wireTicketGroupSettings(root: HTMLElement, bindings: TicketGroupSettingsBindings): () => void {
  const lifetime = createScope();
  const each = (selector: string, run: (group: string) => void) => {
    lifetime.add(
      delegate(root, 'click', selector, (_event, target) => {
        const group = groupOf(target);
        if (group) run(group);
      }),
    );
  };
  lifetime.add(
    delegate(root, 'click', TICKET_GROUP_SETTINGS_ACTIONS.newSettingsGroup.selector, () => {
      bindings.newGroup();
    }),
  );
  each(TICKET_GROUP_SETTINGS_ACTIONS.viewGroupTickets.selector, (group) => {
    bindings.viewTickets(group);
  });
  each(TICKET_GROUP_SETTINGS_ACTIONS.renameSettingsGroup.selector, (group) => {
    bindings.rename(group);
  });
  each(TICKET_GROUP_SETTINGS_ACTIONS.mergeSettingsGroup.selector, (group) => {
    bindings.merge(group);
  });
  each(TICKET_GROUP_SETTINGS_ACTIONS.deleteSettingsGroup.selector, (group) => {
    bindings.remove(group);
  });
  lifetime.add(
    delegate(root, 'change', TICKET_GROUP_TARGETS.groupMergeInto.selector, (_event, target) => {
      bindings.chooseMergeDestination((target as HTMLSelectElement).value);
    }),
  );
  lifetime.add(
    delegate(root, 'submit', TICKET_GROUP_SETTINGS_ACTIONS.submitGroupMerge.selector, (event) => {
      event.preventDefault();
      bindings.submitMerge();
    }),
  );
  lifetime.add(
    delegate(root, 'click', TICKET_GROUP_SETTINGS_ACTIONS.cancelGroupMerge.selector, () => {
      bindings.cancelMerge();
    }),
  );
  lifetime.add(
    delegate(root, 'kui-request-close', TICKET_GROUP_TARGETS.groupMergeDialog.selector, (event, target) => {
      // Only the dialog's own close request counts.
      if (event.target !== target) return;
      bindings.cancelMerge();
    }),
  );
  return () => {
    lifetime.dispose();
  };
}
