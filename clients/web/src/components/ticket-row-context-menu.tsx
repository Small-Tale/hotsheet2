import '@awesome.me/webawesome/dist/components/dropdown-item/dropdown-item.js';
import '@awesome.me/webawesome/dist/components/divider/divider.js';
import { Archive, CircleDot, Copy, Gauge, Shapes, SquareArrowOutUpRight, Star, Tag, Trash2, type IconNode } from 'lucide';
import { LucideIcon } from './lucide-icon';
import './ticket-row-context-menu.css';

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

function ContextItem({ item }: { item: typeof TICKET_CONTEXT_ACTIONS[number] }) {
  return <wa-dropdown-item data-context-action={item.action} variant={item.danger ? 'danger' : undefined}>
    <span slot="icon" class="ticket-context-menu__icon"><LucideIcon icon={item.icon} name={item.iconName} /></span>
    {item.action}
  </wa-dropdown-item>;
}

export function TicketRowContextMenu({ x, y }: { x: number; y: number }) {
  return <div class="ticket-context-menu" role="menu" aria-label="Ticket actions" style={`left:${x}px;top:${y}px`}>
    <ContextItem item={TICKET_CONTEXT_ACTIONS[0]} />
    <wa-divider></wa-divider>
    {TICKET_CONTEXT_ACTIONS.slice(1, 5).map(item => <ContextItem item={item} />)}
    <wa-divider></wa-divider>
    {TICKET_CONTEXT_ACTIONS.slice(5).map(item => <ContextItem item={item} />)}
  </div>;
}
