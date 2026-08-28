import { ChevronDown, ChevronUp, ChevronsUp, Minus, type IconNode } from 'lucide';
import type { TicketPriority } from './ticket-row';
import { Select } from './select';

const PRIORITIES: readonly { value: TicketPriority; label: string; color: string; icon: IconNode; iconName: string }[] = [
  { value: 'urgent', label: 'Urgent', color: '#ef4444', icon: ChevronsUp, iconName: 'chevrons-up' },
  { value: 'high', label: 'High', color: '#f97316', icon: ChevronUp, iconName: 'chevron-up' },
  { value: 'default', label: 'Default', color: '#6b7280', icon: Minus, iconName: 'minus' },
  { value: 'low', label: 'Low', color: '#3b82f6', icon: ChevronDown, iconName: 'chevron-down' },
];
export interface TicketPrioritySelectProps { name: string; value: TicketPriority; label?: string }
export function TicketPrioritySelect({ name, value, label = 'Priority' }: TicketPrioritySelectProps) {
  return <Select className="ticket-priority-select" name={name} value={value} label={label} choices={PRIORITIES} />;
}
