import './ticket-group.css';

import { ListHeader } from '@kerfjs/ui/list-header';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { SunkenPanel } from '@kerfjs/ui/sunken-panel';
import type { SafeHtml } from 'kerfjs/jsx-runtime';
import { Ellipsis } from 'lucide';

import { TICKET_GROUP_ACTIONS } from '../interaction-attrs/ticket-groups';
import { ticketGroupDomId, ticketGroupKey } from '../ticket-groups';
import type { TicketRowPresentation } from './ticket-row';

export interface TicketGroupProps {
  /** The group's stored value: its id, or a legacy group's name (HS2-9KZSD4). */
  group: string;
  /** The group's display name. */
  name: string;
  /** How many of the group's tickets this container holds (one column's share on the board). */
  count: number;
  /** Which collection the container sits in: `list`, or a board column id. */
  scope: string;
  presentation?: TicketRowPresentation;
  /** A collapsed group keeps its header and count and hides its tickets. */
  collapsed?: boolean;
  /** A dragged ticket is over this group. */
  dropTarget?: boolean;
  children?: SafeHtml | SafeHtml[];
}

/**
 * One ticket group in a list or a board column (HS2-WZ3JGZ, HS2-HV8S4Y): a lowered Kerf panel
 * holding a disclosure header and the group's tickets. The same group renders once per column.
 * The header's label and arrow toggle the group; its trailing overflow button opens the group
 * header menu and never toggles, and its label starts on the ticket rows' leading-content edge
 * (HS2-HKXSB1).
 */
export function TicketGroup({
  group,
  name,
  count,
  scope,
  presentation = 'list',
  collapsed = false,
  dropTarget = false,
  children,
}: TicketGroupProps) {
  const key = ticketGroupKey(group),
    region = ticketGroupDomId(scope, group);
  return (
    <div
      class="ticket-group"
      data-key={`ticket-group:${scope}:${key}`}
      data-component="ticket-group"
      data-presentation={presentation}
      data-ticket-group={group}
      data-ticket-group-scope={scope}
      data-ticket-drop-group={group}
      data-collapsed={String(collapsed)}
      data-drop-target={dropTarget ? 'true' : undefined}
      role="group"
      aria-label={`${name} group`}
    >
      <SunkenPanel outlined={dropTarget}>
        <ListHeader
          label={name}
          size="m"
          wrap
          leadingEdge="summary-row"
          toggle
          action={TICKET_GROUP_ACTIONS.toggleTicketGroup.value}
          expanded={!collapsed}
          count={count}
          countLabel={`${count} ticket${count === 1 ? '' : 's'}`}
          controls={region}
          triggerAttributes={{ 'data-ticket-group': group }}
          overflowAction={TICKET_GROUP_ACTIONS.openTicketGroupMenu.value}
          overflowLabel={`${name} Group Actions`}
          overflowIcon={<LucideIcon size="s" icon={Ellipsis} name="ellipsis" />}
          overflowTriggerAttributes={{ 'data-ticket-group': group, 'aria-haspopup': 'menu' }}
        />
        {!collapsed && (
          <div id={region} class="ticket-group__tickets">
            {children}
          </div>
        )}
      </SunkenPanel>
    </div>
  );
}
