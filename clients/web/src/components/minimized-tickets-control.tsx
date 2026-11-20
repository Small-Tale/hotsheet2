import { Badge } from '@kerfjs/ui/badge';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { PopupMenu, type PopupMenuEntry } from '@kerfjs/ui/popup-menu';
import { ToolbarControlGroup } from '@kerfjs/ui/toolbar-control-group';
import { delegate } from 'kerfjs';
import { createScope } from 'kerfjs/scope';
import { CircleAlert, Layers2, X } from 'lucide';

import { INSPECTOR_AND_EDITOR_ACTIONS } from '../interaction-attrs/inspector-and-editor';
import { defaultCategoryPresentation } from './category-presentation';

/** One minimized ticket reader as the floating control lists it (HS2-Q8WF64). */
export interface MinimizedTicketSummary {
  /** The minimized reader frame id the restore action names. */
  id: string;
  slug: string;
  title: string;
  category: string;
  /** The draft indicator, such as "Unsent note", when closing this reader would lose a draft. */
  draft?: string;
}

export interface MinimizedTicketsControlProps {
  tickets: readonly MinimizedTicketSummary[];
}

/** The control's Lucide icon everywhere it stands for minimized tickets, chosen by the maintainer (HS2-Q8WF64). */
const MINIMIZED_ICON = <LucideIcon icon={Layers2} name="layers-2" />;

function ticketLabel(ticket: MinimizedTicketSummary) {
  return `${ticket.slug} ${ticket.title}`.trim();
}

function categoryIcon(category: string) {
  const presentation = defaultCategoryPresentation(category);
  return presentation ? (
    <LucideIcon icon={presentation.icon} name={presentation.iconName} />
  ) : (
    <LucideIcon icon={CircleAlert} name="circle-alert" />
  );
}

/**
 * The minimized ticket readers of the current project tab (HS2-Q8WF64), as one button in the
 * workspace's floating control cluster beside the Terminal Drawer button. One minimized ticket
 * restores on click. Several show a corner count and open a menu that lists each ticket with its
 * draft indicator, then Restore All and Close All…. Renders nothing while no ticket is minimized.
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
          data-minimized-reader-id={only.id}
          data-minimized-tickets-control="single"
          aria-label={`Restore ${ticketLabel(only)}${only.draft ? ` (${only.draft})` : ''}`}
          title={`Restore ${only.slug}`}
        >
          {MINIMIZED_ICON}
        </button>
      </ToolbarControlGroup>
    );
  const items: PopupMenuEntry[] = [
    { kind: 'heading', label: 'Minimized Tickets' },
    ...tickets.map((ticket): PopupMenuEntry => ({
      label: ticketLabel(ticket),
      action: INSPECTOR_AND_EDITOR_ACTIONS.restoreMinimizedTicket.value,
      icon: categoryIcon(ticket.category),
      details: ticket.draft ? (
        <Badge tone="warning" appearance="solid" size="compact">
          {ticket.draft}
        </Badge>
      ) : undefined,
      attributes: { 'data-minimized-reader-id': ticket.id },
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

/** Wire every MinimizedTicketsControl under `root`; production restores readers, the demo a fixture. */
export function wireMinimizedTicketsControl(root: HTMLElement, bindings: MinimizedTicketsControlBindings): () => void {
  const lifetime = createScope();
  lifetime.add(
    delegate(root, 'click', INSPECTOR_AND_EDITOR_ACTIONS.restoreMinimizedTicket.selector, (_event, target) => {
      const id = (target as HTMLElement).dataset.minimizedReaderId;
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
