import './ticket-group.css';

import { ListHeader } from '@kerfjs/ui/list-header';
import { SunkenPanel } from '@kerfjs/ui/sunken-panel';
import type { SafeHtml } from 'kerfjs/jsx-runtime';

import { TICKET_GROUP_ACTIONS } from '../interaction-attrs/ticket-groups';
import { ticketGroupDomId, ticketGroupKey } from '../ticket-groups';
import type { TicketRowPresentation } from './ticket-row';

export interface TicketGroupProps {
  /** The group's name, in the user's casing. */
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
 */
export function TicketGroup({
  name,
  count,
  scope,
  presentation = 'list',
  collapsed = false,
  dropTarget = false,
  children,
}: TicketGroupProps) {
  const key = ticketGroupKey(name),
    region = ticketGroupDomId(scope, name);
  return (
    <div
      class="ticket-group"
      data-key={`ticket-group:${scope}:${key}`}
      data-component="ticket-group"
      data-presentation={presentation}
      data-ticket-group={name}
      data-ticket-group-scope={scope}
      data-ticket-drop-group={name}
      data-collapsed={String(collapsed)}
      data-drop-target={dropTarget ? 'true' : undefined}
      role="group"
      aria-label={`${name} group`}
    >
      <div class="ticket-group__frame">
        <SunkenPanel outlined={dropTarget}>
          <ListHeader
            label={name}
            size="m"
            wrap
            toggle
            action={TICKET_GROUP_ACTIONS.toggleTicketGroup.value}
            expanded={!collapsed}
            count={count}
            countLabel={`${count} ticket${count === 1 ? '' : 's'}`}
            controls={region}
            triggerAttributes={{ 'data-ticket-group': name }}
          />
          {!collapsed && (
            <div id={region} class="ticket-group__tickets">
              {children}
            </div>
          )}
        </SunkenPanel>
      </div>
    </div>
  );
}
