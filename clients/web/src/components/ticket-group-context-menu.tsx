import { uiColor } from '@kerfjs/ui/css-values';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { PopupMenu, type PopupMenuEntry } from '@kerfjs/ui/popup-menu';
import { CheckCheck, Merge, Pencil, Ungroup } from 'lucide';

import { contextMenuAnchor } from '../context-menu-position';
import { TICKET_GROUP_ACTIONS } from '../interaction-attrs/ticket-groups';

export interface TicketGroupContextMenuProps {
  x: number;
  y: number;
  /** The stored value of the group the menu was opened on (HS2-9KZSD4). */
  group: string;
  /** Which container the menu was opened on: `list`, or a board column id (HS2-EYEGDQ). */
  scope?: string;
  /** The group's display name; defaults to the stored value. */
  name?: string;
  /** Other groups share this group's name, so Merge Duplicate Groups applies (HS2-9KZSD4). */
  mergeable?: boolean;
  /** Why the group cannot be changed, such as a provider without group support. */
  disabledReason?: string;
}

/**
 * The group header menu (HS2-8CDFKR): select the tickets the group shows where it was opened
 * (HS2-EYEGDQ), rename the group or ungroup all of its tickets, and merge
 * same-named duplicate groups into it when it has any (HS2-9KZSD4). It opens
 * from a right-click or the keyboard menu key on a group header, because Kerf's ListHeader
 * cannot yet pair its toggle with a separate trailing action.
 */
export function TicketGroupContextMenu({
  x,
  y,
  group,
  scope = 'list',
  name = group,
  mergeable = false,
  disabledReason,
}: TicketGroupContextMenuProps) {
  const disabled = Boolean(disabledReason),
    entries: PopupMenuEntry[] = [
      {
        // Selection changes no ticket, so a provider without group support still allows it.
        label: 'Select All in Group',
        action: TICKET_GROUP_ACTIONS.selectTicketGroup.value,
        icon: <LucideIcon size="s" icon={CheckCheck} name="check-check" color={uiColor('neutral-on-quiet')} />,
        attributes: { 'data-ticket-group': group, 'data-ticket-group-scope': scope },
      },
      { kind: 'divider' },
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
      ...(mergeable
        ? [
            {
              label: 'Merge Duplicate Groups',
              action: TICKET_GROUP_ACTIONS.mergeTicketGroups.value,
              disabled,
              disabledReason,
              icon: <LucideIcon size="s" icon={Merge} name="merge" color={uiColor('neutral-on-quiet')} />,
              attributes: { 'data-ticket-group': group },
            },
          ]
        : []),
    ];
  return (
    <PopupMenu
      context
      anchor={contextMenuAnchor(x, y)}
      label={`${name} Group Actions`}
      // The shell reveals and dismisses this menu as its `ticket` context surface.
      rootAttributes={{ 'data-context-menu': 'ticket', 'data-ticket-group-menu': 'true' }}
      items={entries}
    />
  );
}
