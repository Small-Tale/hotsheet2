import '@awesome.me/webawesome/dist/components/dropdown/dropdown.js';
import '@awesome.me/webawesome/dist/components/dropdown-item/dropdown-item.js';
import '@awesome.me/webawesome/dist/components/divider/divider.js';
import './ticket-row-context-menu.css';

import { Archive, BadgeCheck, CircleDot, CircleX, Clock3, Copy, Gauge, type IconNode,Shapes, SquareArrowOutUpRight, Star, Tag, Tags, Trash2 } from 'lucide';

import { DEFAULT_TICKET_CATEGORIES } from './category-presentation';
import { LucideIcon } from './lucide-icon';
import type { TicketStatus } from './status-badge';
import { getPriorityPresentation, type TicketPriority } from './ticket-row';
import { TICKET_STATUS_CHOICES } from './ticket-status-menu';

export const TICKET_CONTEXT_ACTIONS: ReadonlyArray<{ action: string; icon: IconNode; iconName: string; danger?: boolean }> = [
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

function ContextItem({ item, disabled = false }: { item: { action: string; label?: string; icon: IconNode; iconName: string; danger?: boolean }; disabled?: boolean }) {
  return <wa-dropdown-item data-context-action={item.action} variant={item.danger ? 'danger' : undefined} disabled={disabled} title={disabled ? 'One or more selected ticket providers do not support updates.' : undefined}>
    <span slot="icon" class="ticket-context-menu__icon"><LucideIcon icon={item.icon} name={item.iconName} /></span>
    {item.label ?? item.action}
  </wa-dropdown-item>;
}

const PRIORITIES: readonly { value: TicketPriority; label: string }[] = [{ value: 'urgent', label: 'Urgent' }, { value: 'high', label: 'High' }, { value: 'default', label: 'Default' }, { value: 'low', label: 'Low' }];
function MetadataSubmenu({ field, label, icon, iconName, choices, selected, disabled = false }: { field: 'category' | 'priority' | 'status'; label: string; icon: IconNode; iconName: string; choices: readonly { value: string; label: string; icon: IconNode; iconName: string; color?: string; separatorBefore?: boolean }[]; selected?: string; disabled?: boolean }) {
  return <wa-dropdown-item disabled={disabled} title={disabled ? 'One or more selected ticket providers do not support updates.' : undefined}>
    <span slot="icon" class="ticket-context-menu__icon"><LucideIcon icon={icon} name={iconName} /></span>{label}
    {choices.map(choice => <>{choice.separatorBefore && <wa-divider slot="submenu"></wa-divider>}<wa-dropdown-item slot="submenu" type="checkbox" checked={choice.value === selected} data-context-field={field} data-context-value={choice.value} value={choice.value}><span slot="icon" class="ticket-context-menu__icon" style={choice.color ? `color:${choice.color}` : undefined}><LucideIcon icon={choice.icon} name={choice.iconName} /></span>{choice.label}</wa-dropdown-item></>)}
  </wa-dropdown-item>;
}

export interface TicketRowContextMenuProps { x: number; y: number; category?: string; priority?: TicketPriority; status?: TicketStatus; upNextEligible?: boolean; verifyAction?: boolean; notWorkingAction?: boolean; selectionCount?: number; canBulkUpdate?: boolean }
export function TicketRowContextMenu({ x, y, category, priority, status, upNextEligible = true, verifyAction = false, notWorkingAction = false, selectionCount = 1, canBulkUpdate = true }: TicketRowContextMenuProps) {
  const priorityChoices = PRIORITIES.map(choice => { const option = getPriorityPresentation(choice.value); return { ...choice, icon: option.icon, iconName: option.name, color: option.color }; });
  return <div class="ticket-context-menu" role="menu" aria-label="Ticket actions" style={`left:${x}px;top:${y}px`}>
    <wa-dropdown open placement="bottom-start" distance={0}>
      <span slot="trigger" class="ticket-context-menu__anchor" aria-hidden="true"></span>
      {(verifyAction || notWorkingAction) && <>{verifyAction && <ContextItem item={COMPLETED_TICKET_CONTEXT_ACTIONS[0]} />}{notWorkingAction && <ContextItem item={COMPLETED_TICKET_CONTEXT_ACTIONS[1]} />}<wa-divider></wa-divider></>}
      {/* "Open ticket" opens a single ticket, so hide it when several are selected (HS2-XRENF2). */}
      {selectionCount <= 1 && <><ContextItem item={TICKET_CONTEXT_ACTIONS[0]} /><wa-divider></wa-divider></>}
      <MetadataSubmenu field="category" label="Change category" icon={Shapes} iconName="shapes" choices={DEFAULT_TICKET_CATEGORIES} selected={category} disabled={!canBulkUpdate} />
      <MetadataSubmenu field="priority" label="Change priority" icon={Gauge} iconName="gauge" choices={priorityChoices} selected={priority} disabled={!canBulkUpdate} />
      <MetadataSubmenu field="status" label="Change status" icon={CircleDot} iconName="circle-dot" choices={TICKET_STATUS_CHOICES} selected={status} disabled={!canBulkUpdate} />
      {upNextEligible && <ContextItem item={TICKET_CONTEXT_ACTIONS[4]} disabled={!canBulkUpdate} />}
      <wa-divider></wa-divider>
      {TICKET_CONTEXT_ACTIONS.slice(5).map(item => <ContextItem item={item} disabled={!canBulkUpdate && item.action !== 'Duplicate ticket'} />)}
    </wa-dropdown>
  </div>;
}
