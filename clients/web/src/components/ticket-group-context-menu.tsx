import { uiColor } from '@kerfjs/ui/css-values';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { PopupMenu, type PopupMenuEntry } from '@kerfjs/ui/popup-menu';
import { Pencil, Ungroup } from 'lucide';

import { contextMenuAnchor } from '../context-menu-position';
import { TICKET_GROUP_ACTIONS } from '../interaction-attrs/ticket-groups';

export interface TicketGroupContextMenuProps {
  x: number;
  y: number;
  /** The group the menu was opened on. */
  group: string;
  /** Why the group cannot be changed, such as a provider without group support. */
  disabledReason?: string;
}

/**
 * The group header menu (HS2-8CDFKR): rename the group or ungroup all of its tickets. It opens
 * from a right-click or the keyboard menu key on a group header, because Kerf's ListHeader
 * cannot yet pair its toggle with a separate trailing action.
 */
export function TicketGroupContextMenu({ x, y, group, disabledReason }: TicketGroupContextMenuProps) {
  const disabled = Boolean(disabledReason),
    entries: PopupMenuEntry[] = [
      {
        label: 'Rename Group…',
        action: TICKET_GROUP_ACTIONS.renameTicketGroup.value,
        disabled,
        disabledReason,
        icon: <LucideIcon size="s" icon={Pencil} name="pencil" color={uiColor('neutral-on-quiet')} />,
        attributes: { 'data-ticket-group': group },
      },
      {
        label: 'Ungroup',
        action: TICKET_GROUP_ACTIONS.ungroupTicketGroup.value,
        disabled,
        disabledReason,
        icon: <LucideIcon size="s" icon={Ungroup} name="ungroup" color={uiColor('neutral-on-quiet')} />,
        attributes: { 'data-ticket-group': group },
      },
    ];
  return (
    <PopupMenu
      context
      anchor={contextMenuAnchor(x, y)}
      label={`${group} Group Actions`}
      // The shell reveals and dismisses this menu as its `ticket` context surface.
      rootAttributes={{ 'data-context-menu': 'ticket', 'data-ticket-group-menu': 'true' }}
      items={entries}
    />
  );
}
