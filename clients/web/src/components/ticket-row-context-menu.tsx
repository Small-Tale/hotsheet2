import './ticket-row-context-menu.css';

import { foregroundColor, uiColor } from '@kerfjs/ui/css-values';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { PopupMenu, type PopupMenuEntry, type PopupMenuItem } from '@kerfjs/ui/popup-menu';
import {
  Archive,
  ArchiveRestore,
  BadgeCheck,
  CircleDot,
  CircleX,
  Clock3,
  Copy,
  Gauge,
  type IconNode,
  RotateCcw,
  Shapes,
  SquareArrowOutUpRight,
  Star,
  Tag,
  Tags,
  Trash2,
  XCircle,
} from 'lucide';

import { contextPopupMenuAnchor } from '../context-menu-position';
import { DEFAULT_TICKET_CATEGORIES } from './category-presentation';
import type { TicketStatus } from './status-badge';
import { getPriorityPresentation, type TicketPriority } from './ticket-row';
import { TICKET_STATUS_CHOICES } from './ticket-status-menu';

export const TICKET_CONTEXT_ACTIONS: ReadonlyArray<{
  action: string;
  icon: IconNode;
  iconName: string;
  danger?: boolean;
}> = [
  { action: 'Open ticket', icon: SquareArrowOutUpRight, iconName: 'square-arrow-out-up-right' },
  { action: 'Change category', icon: Shapes, iconName: 'shapes' },
  { action: 'Change priority', icon: Gauge, iconName: 'gauge' },
  { action: 'Change status', icon: CircleDot, iconName: 'circle-dot' },
  { action: 'Toggle Up Next', icon: Star, iconName: 'star' },
  { action: 'Add tag', icon: Tag, iconName: 'tag' },
  { action: 'Remove tag', icon: Tags, iconName: 'tags' },
  { action: 'Duplicate ticket', icon: Copy, iconName: 'copy' },
  { action: 'Move to Backlog', icon: Clock3, iconName: 'clock-3' },
  { action: 'Archive ticket', icon: Archive, iconName: 'archive' },
  { action: 'Delete ticket', icon: Trash2, iconName: 'trash-2', danger: true },
];

export const COMPLETED_TICKET_CONTEXT_ACTIONS = [
  { action: 'Verify ticket', label: 'Verified', icon: BadgeCheck, iconName: 'badge-check' },
  { action: 'Report not working', label: 'Not Working…', icon: CircleX, iconName: 'circle-x' },
] as const;

export const RESTORE_TICKET_CONTEXT_ACTION = {
  action: 'Restore ticket',
  label: 'Restore from Trash',
  icon: ArchiveRestore,
  iconName: 'archive-restore',
} as const;

export const REOPEN_TICKET_CONTEXT_ACTION = {
  action: 'Reopen ticket',
  label: 'Reopen Ticket',
  icon: RotateCcw,
  iconName: 'rotate-ccw',
} as const;

export const CLOSE_TICKET_CONTEXT_ACTION = {
  action: 'Close ticket',
  label: 'Close ticket…',
  icon: XCircle,
  iconName: 'x-circle',
} as const;

function contextEntry(
  item: { action: string; label?: string; icon: IconNode; iconName: string; danger?: boolean },
  disabled = false,
  disabledTitle?: string,
): PopupMenuItem {
  return {
    label: item.label ?? item.action,
    tone: item.danger ? 'danger' : 'default',
    disabled,
    disabledReason: disabled
      ? (disabledTitle ?? 'One or more selected ticket providers do not support updates.')
      : undefined,
    icon: (
      <span class="ticket-context-menu__icon">
        <LucideIcon
          size="s"
          icon={item.icon}
          name={item.iconName}
          color={item.danger ? uiColor('danger-on-quiet') : uiColor('neutral-on-quiet')}
        />
      </span>
    ),
    attributes: { 'data-context-action': item.action },
  };
}

const PRIORITIES: readonly { value: TicketPriority; label: string }[] = [
  { value: 'urgent', label: 'Urgent' },
  { value: 'high', label: 'High' },
  { value: 'default', label: 'Default' },
  { value: 'low', label: 'Low' },
];
function metadataSubmenu({
  field,
  label,
  icon,
  iconName,
  choices,
  selected,
  disabled = false,
}: {
  field: 'category' | 'priority' | 'status';
  label: string;
  icon: IconNode;
  iconName: string;
  choices: readonly {
    value: string;
    label: string;
    icon?: IconNode;
    iconName?: string;
    color?: string;
    separatorBefore?: boolean;
  }[];
  selected?: string;
  disabled?: boolean;
}): PopupMenuItem {
  return {
    label,
    disabled,
    disabledReason: disabled ? 'One or more selected ticket providers do not support updates.' : undefined,
    icon: (
      <span class="ticket-context-menu__icon">
        <LucideIcon size="s" icon={icon} name={iconName} color={uiColor('neutral-on-quiet')} />
      </span>
    ),
    // PopupMenu submenus hold items only, so a choice's `separatorBefore` has no counterpart here
    // (Kerf gap KF-7KR1BC).
    submenu: choices.map((choice) => ({
      label: choice.label,
      value: choice.value,
      checked: choice.value === selected,
      icon:
        choice.icon && choice.iconName ? (
          <span class="ticket-context-menu__icon">
            <LucideIcon
              size="s"
              icon={choice.icon}
              name={choice.iconName}
              color={choice.color ? foregroundColor(choice.color) : uiColor('neutral-on-quiet')}
            />
          </span>
        ) : undefined,
      attributes: { 'data-context-field': field, 'data-context-value': choice.value },
    })),
  };
}

export interface TicketRowContextMenuProps {
  x: number;
  y: number;
  category?: string;
  priority?: TicketPriority;
  status?: TicketStatus;
  upNextEligible?: boolean;
  hideUpNext?: boolean;
  verifyAction?: boolean;
  notWorkingAction?: boolean;
  reopenAction?: boolean;
  closeAction?: boolean;
  selectionCount?: number;
  canBulkUpdate?: boolean;
  allInBacklog?: boolean;
  allInArchive?: boolean;
  allInTrash?: boolean;
}

/** Shadow-DOM-safe containment check for capture-phase context-menu dismissal. */
export function eventTargetsContextMenu(
  event: { composedPath(): unknown[] },
  selector = '.ticket-context-menu',
): boolean {
  return event.composedPath().some((node) => {
    const candidate = node as { matches?: (value: string) => boolean };
    return typeof candidate.matches === 'function' && candidate.matches(selector);
  });
}

export function TicketRowContextMenu({
  x,
  y,
  category,
  priority,
  status,
  upNextEligible = true,
  hideUpNext = false,
  verifyAction = false,
  notWorkingAction = false,
  reopenAction = false,
  closeAction = false,
  selectionCount = 1,
  canBulkUpdate = true,
  allInBacklog = false,
  allInArchive = false,
  allInTrash = false,
}: TicketRowContextMenuProps) {
  const priorityChoices = PRIORITIES.map((choice) => {
    const option = getPriorityPresentation(choice.value);
    return { ...choice, icon: option.icon, iconName: option.name, color: option.color };
  });
  const entries: PopupMenuEntry[] = [];
  if (allInTrash) entries.push(contextEntry(RESTORE_TICKET_CONTEXT_ACTION, !canBulkUpdate), { kind: 'divider' });
  if (reopenAction) entries.push(contextEntry(REOPEN_TICKET_CONTEXT_ACTION, !canBulkUpdate), { kind: 'divider' });
  if (verifyAction || notWorkingAction) {
    if (verifyAction) entries.push(contextEntry(COMPLETED_TICKET_CONTEXT_ACTIONS[0]));
    if (notWorkingAction) entries.push(contextEntry(COMPLETED_TICKET_CONTEXT_ACTIONS[1]));
    entries.push({ kind: 'divider' });
  }
  // "Open ticket" opens a single ticket, so hide it when several are selected (HS2-XRENF2).
  if (selectionCount <= 1) entries.push(contextEntry(TICKET_CONTEXT_ACTIONS[0]), { kind: 'divider' });
  entries.push(
    metadataSubmenu({
      field: 'category',
      label: 'Change category',
      icon: Shapes,
      iconName: 'shapes',
      choices: DEFAULT_TICKET_CATEGORIES,
      selected: category,
      disabled: !canBulkUpdate,
    }),
    metadataSubmenu({
      field: 'priority',
      label: 'Change priority',
      icon: Gauge,
      iconName: 'gauge',
      choices: priorityChoices,
      selected: priority,
      disabled: !canBulkUpdate,
    }),
    metadataSubmenu({
      field: 'status',
      label: 'Change status',
      icon: CircleDot,
      iconName: 'circle-dot',
      choices: TICKET_STATUS_CHOICES,
      selected: status,
      disabled: !canBulkUpdate,
    }),
  );
  if (upNextEligible && !hideUpNext) entries.push(contextEntry(TICKET_CONTEXT_ACTIONS[4], !canBulkUpdate));
  entries.push({ kind: 'divider' });
  if (closeAction) entries.push(contextEntry(CLOSE_TICKET_CONTEXT_ACTION), { kind: 'divider' });
  for (const item of TICKET_CONTEXT_ACTIONS.slice(5)) {
    const alreadyThere =
      (item.action === 'Move to Backlog' && allInBacklog) || (item.action === 'Archive ticket' && allInArchive);
    entries.push(
      contextEntry(
        item,
        alreadyThere || (!canBulkUpdate && item.action !== 'Duplicate ticket'),
        alreadyThere ? `Every selected ticket is already in ${allInBacklog ? 'Backlog' : 'Archive'}.` : undefined,
      ),
    );
  }
  return (
    <div
      class="ticket-context-menu ticket-row-context-menu"
      role="menu"
      aria-label="Ticket actions"
      {...contextPopupMenuAnchor(x, y)}
    >
      <PopupMenu context label="Ticket actions" rootAttributes={{ 'data-context-menu': 'ticket' }} items={entries} />
    </div>
  );
}
