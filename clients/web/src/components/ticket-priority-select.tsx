import '@awesome.me/webawesome/dist/components/option/option.js';
import '@awesome.me/webawesome/dist/components/select/select.js';
import { ChevronDown, ChevronUp, ChevronsUp, Minus, type IconNode } from 'lucide';
import type { TicketPriority } from './ticket-row';
import { LucideIcon } from './lucide-icon';
import './ticket-priority-select.css';

const PRIORITIES: readonly { value: TicketPriority; label: string; color: string; icon: IconNode; iconName: string }[] = [
  { value: 'urgent', label: 'Urgent', color: '#ef4444', icon: ChevronsUp, iconName: 'chevrons-up' },
  { value: 'high', label: 'High', color: '#f97316', icon: ChevronUp, iconName: 'chevron-up' },
  { value: 'default', label: 'Default', color: '#6b7280', icon: Minus, iconName: 'minus' },
  { value: 'low', label: 'Low', color: '#3b82f6', icon: ChevronDown, iconName: 'chevron-down' },
];
export interface TicketPrioritySelectProps { name: string; value: TicketPriority; label?: string }
export function TicketPrioritySelect({ name, value, label = 'Priority' }: TicketPrioritySelectProps) {
  return <wa-select class="ticket-priority-select" name={name} label={label} value={value}>{PRIORITIES.map(priority => <wa-option value={priority.value}><span slot="start" class="ticket-priority-select__icon" style={`color:${priority.color}`}><LucideIcon icon={priority.icon} name={priority.iconName} /></span>{priority.label}</wa-option>)}</wa-select>;
}
