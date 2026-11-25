import { Badge } from '@kerfjs/ui/badge';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { PopupMenu, type PopupMenuEntry } from '@kerfjs/ui/popup-menu';
import { ToolbarControlGroup } from '@kerfjs/ui/toolbar-control-group';
import { delegate } from 'kerfjs';
import { createScope } from 'kerfjs/scope';
import { Layers2, X } from 'lucide';

import { INSPECTOR_AND_EDITOR_ACTIONS } from '../interaction-attrs/inspector-and-editor';
import type { MinimizedDialogItem } from '../minimized-dialogs';

/**
 * One minimized dialog as the floating control lists it: a ticket reader (HS2-Q8WF64) or another
 * minimized dialog kind such as the new-ticket composer (HS2-3JR0ZH). `minimizedAt` is optional
 * here because the control lists items in the order it is given.
 */
export type MinimizedTicketSummary = Omit<MinimizedDialogItem, 'minimizedAt'> & { minimizedAt?: number };

export interface MinimizedTicketsControlProps {
  tickets: readonly MinimizedTicketSummary[];
}

/** The control's Lucide icon everywhere it stands for minimized tickets, chosen by the maintainer (HS2-Q8WF64). */
const MINIMIZED_ICON = <LucideIcon icon={Layers2} name="layers-2" />;

/**
 * The minimized dialogs of the current project tab (ticket readers, HS2-Q8WF64, and the new-ticket
 * composer, HS2-3JR0ZH), as one button in the workspace's floating control cluster beside the
 * Terminal Drawer button. One minimized item restores on click. Several show a corner count and
 * open a menu that lists each item with its draft indicator, then Restore All and Close All….
 * Renders nothing while nothing is minimized.
 */
export function MinimizedTicketsControl({ tickets }: MinimizedTicketsControlProps) {
  const only = tickets.at(0);
  if (!only) return null;
  if (tickets.length === 1)
    return (
      <ToolbarControlGroup single label="Minimized Tickets">
        <button
          type="button"
          {...INSPECTOR_AND_EDITOR_ACTIONS.restoreMinimizedTicket.attrs}
          data-minimized-item-id={only.id}
          data-minimized-kind={only.kind}
          data-minimized-tickets-control="single"
          aria-label={`Restore ${only.label}${only.draft ? ` (${only.draft})` : ''}`}
          title={`Restore ${only.shortLabel}`}
        >
          {MINIMIZED_ICON}
        </button>
      </ToolbarControlGroup>
    );
  const items: PopupMenuEntry[] = [
    { kind: 'heading', label: 'Minimized Tickets' },
    ...tickets.map((ticket): PopupMenuEntry => ({
      label: ticket.label,
      action: INSPECTOR_AND_EDITOR_ACTIONS.restoreMinimizedTicket.value,
      icon: <LucideIcon icon={ticket.icon.icon} name={ticket.icon.name} />,
      details: ticket.draft ? (
        <Badge tone="warning" appearance="solid" size="compact">
          {ticket.draft}
        </Badge>
      ) : undefined,
      attributes: { 'data-minimized-item-id': ticket.id, 'data-minimized-kind': ticket.kind },
    })),
    { kind: 'divider' },
    {
      label: 'Restore All',
      action: INSPECTOR_AND_EDITOR_ACTIONS.restoreAllMinimizedTickets.value,
      icon: MINIMIZED_ICON,
    },
    {
      label: tickets.some((ticket) => ticket.draft) ? 'Close All…' : 'Close All',
      action: INSPECTOR_AND_EDITOR_ACTIONS.closeAllMinimizedTickets.value,
      icon: <LucideIcon icon={X} name="x" />,
    },
  ];
  return (
    <ToolbarControlGroup single nestedDropdown label="Minimized Tickets">
      <PopupMenu
        label={`${tickets.length} Minimized Tickets`}
        icon={
          <>
            {MINIMIZED_ICON}
            <Badge placement="corner" tone="neutral" appearance="solid" size="compact" ariaHidden>
              {tickets.length}
            </Badge>
          </>
        }
        caret={false}
        placement="top-end"
        rootAttributes={{ 'data-minimized-tickets-control': 'menu' }}
        items={items}
      />
    </ToolbarControlGroup>
  );
}

/** What the owner does with each minimized-ticket command. */
export interface MinimizedTicketsControlBindings {
  readonly restore: (id: string) => void;
  readonly restoreAll: () => void;
  readonly closeAll: () => void;
}

/** Wire every MinimizedTicketsControl under `root`; production restores dialogs, the demo a fixture. */
export function wireMinimizedTicketsControl(root: HTMLElement, bindings: MinimizedTicketsControlBindings): () => void {
  const lifetime = createScope();
  lifetime.add(
    delegate(root, 'click', INSPECTOR_AND_EDITOR_ACTIONS.restoreMinimizedTicket.selector, (_event, target) => {
      const id = (target as HTMLElement).dataset.minimizedItemId;
      if (id) bindings.restore(id);
    }),
  );
  lifetime.add(
    delegate(root, 'click', INSPECTOR_AND_EDITOR_ACTIONS.restoreAllMinimizedTickets.selector, () => {
      bindings.restoreAll();
    }),
  );
  lifetime.add(
    delegate(root, 'click', INSPECTOR_AND_EDITOR_ACTIONS.closeAllMinimizedTickets.selector, () => {
      bindings.closeAll();
    }),
  );
  return () => {
    lifetime.dispose();
  };
}
