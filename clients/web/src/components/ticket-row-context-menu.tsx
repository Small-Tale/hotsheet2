import '@awesome.me/webawesome/dist/components/dropdown/dropdown.js';
import '@awesome.me/webawesome/dist/components/dropdown-item/dropdown-item.js';
import '@awesome.me/webawesome/dist/components/divider/divider.js';
import './ticket-row-context-menu.css';

import { Archive, BadgeCheck, CircleDot, CircleX, Copy, Gauge, type IconNode,Shapes, SquareArrowOutUpRight, Star, Tag, Trash2 } from 'lucide';

import { DEFAULT_TICKET_CATEGORIES } from './category-presentation';
import { LucideIcon } from './lucide-icon';
import { statusPresentation, type TicketStatus } from './status-badge';
import { getPriorityPresentation, type TicketPriority } from './ticket-row';

export const TICKET_CONTEXT_ACTIONS: ReadonlyArray<{ action: string; icon: IconNode; iconName: string; danger?: boolean }> = [
  { action: 'Open ticket', icon: SquareArrowOutUpRight, iconName: 'square-arrow-out-up-right' },
  { action: 'Change category', icon: Shapes, iconName: 'shapes' },
  { action: 'Change priority', icon: Gauge, iconName: 'gauge' },
  { action: 'Change status', icon: CircleDot, iconName: 'circle-dot' },
  { action: 'Toggle Up Next', icon: Star, iconName: 'star' },
  { action: 'Add tag', icon: Tag, iconName: 'tag' },
  { action: 'Duplicate ticket', icon: Copy, iconName: 'copy' },
  { action: 'Archive ticket', icon: Archive, iconName: 'archive' },
  { action: 'Delete ticket', icon: Trash2, iconName: 'trash-2', danger: true },
];

export const COMPLETED_TICKET_CONTEXT_ACTIONS = [
  { action: 'Verify ticket', label: 'Verified', icon: BadgeCheck, iconName: 'badge-check' },
  { action: 'Report not working', label: 'Not Working…', icon: CircleX, iconName: 'circle-x' },
] as const;

function ContextItem({ item }: { item: { action: string; label?: string; icon: IconNode; iconName: string; danger?: boolean } }) {
  return <wa-dropdown-item data-context-action={item.action} variant={item.danger ? 'danger' : undefined}>
    <span slot="icon" class="ticket-context-menu__icon"><LucideIcon icon={item.icon} name={item.iconName} /></span>
    {item.label ?? item.action}
  </wa-dropdown-item>;
}

const PRIORITIES: readonly { value: TicketPriority; label: string }[] = [{ value: 'urgent', label: 'Urgent' }, { value: 'high', label: 'High' }, { value: 'default', label: 'Default' }, { value: 'low', label: 'Low' }];
const STATUSES: readonly TicketStatus[] = ['not_started', 'started', 'completed', 'verified'];

function MetadataSubmenu({ field, label, icon, iconName, choices, selected }: { field: 'category' | 'priority' | 'status'; label: string; icon: IconNode; iconName: string; choices: readonly { value: string; label: string; icon: IconNode; iconName: string; color?: string }[]; selected?: string }) {
  return <wa-dropdown-item>
    <span slot="icon" class="ticket-context-menu__icon"><LucideIcon icon={icon} name={iconName} /></span>{label}
    {choices.map(choice => <wa-dropdown-item slot="submenu" type="checkbox" checked={choice.value === selected} data-context-field={field} data-context-value={choice.value} value={choice.value}><span slot="icon" class="ticket-context-menu__icon" style={choice.color ? `color:${choice.color}` : undefined}><LucideIcon icon={choice.icon} name={choice.iconName} /></span>{choice.label}</wa-dropdown-item>)}
  </wa-dropdown-item>;
}

export interface TicketRowContextMenuProps { x: number; y: number; category?: string; priority?: TicketPriority; status?: TicketStatus; upNextEligible?: boolean; verifyAction?: boolean; notWorkingAction?: boolean; selectionCount?: number }
export function TicketRowContextMenu({ x, y, category, priority, status, upNextEligible = true, verifyAction = false, notWorkingAction = false, selectionCount = 1 }: TicketRowContextMenuProps) {
  const priorityChoices = PRIORITIES.map(choice => { const option = getPriorityPresentation(choice.value); return { ...choice, icon: option.icon, iconName: option.name, color: option.color }; });
  const statusChoices = STATUSES.map(value => ({ value, ...statusPresentation(value) }));
  return <div class="ticket-context-menu" role="menu" aria-label="Ticket actions" style={`left:${x}px;top:${y}px`}>
    <wa-dropdown open placement="bottom-start" distance={0}>
      <span slot="trigger" class="ticket-context-menu__anchor" aria-hidden="true"></span>
      {(verifyAction || notWorkingAction) && <>{verifyAction && <ContextItem item={COMPLETED_TICKET_CONTEXT_ACTIONS[0]} />}{notWorkingAction && <ContextItem item={COMPLETED_TICKET_CONTEXT_ACTIONS[1]} />}<wa-divider></wa-divider></>}
      {/* "Open ticket" opens a single ticket, so hide it when several are selected (HS2-XRENF2). */}
      {selectionCount <= 1 && <><ContextItem item={TICKET_CONTEXT_ACTIONS[0]} /><wa-divider></wa-divider></>}
      <MetadataSubmenu field="category" label="Change category" icon={Shapes} iconName="shapes" choices={DEFAULT_TICKET_CATEGORIES} selected={category} />
      <MetadataSubmenu field="priority" label="Change priority" icon={Gauge} iconName="gauge" choices={priorityChoices} selected={priority} />
      <MetadataSubmenu field="status" label="Change status" icon={CircleDot} iconName="circle-dot" choices={statusChoices} selected={status} />
      {upNextEligible && <ContextItem item={TICKET_CONTEXT_ACTIONS[4]} />}
      <wa-divider></wa-divider>
      {TICKET_CONTEXT_ACTIONS.slice(5).map(item => <ContextItem item={item} />)}
    </wa-dropdown>
  </div>;
}
