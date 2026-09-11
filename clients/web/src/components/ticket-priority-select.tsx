import { ChevronDown, ChevronsUp, ChevronUp, type IconNode,Minus } from 'lucide';

import { Select } from './select';
import type { TicketPriority } from './ticket-row';

const PRIORITIES: readonly { value: TicketPriority; label: string; color: string; icon: IconNode; iconName: string }[] = [
  { value: 'urgent', label: 'Urgent', color: 'var(--wa-color-danger-fill-loud)', icon: ChevronsUp, iconName: 'chevrons-up' },
  { value: 'high', label: 'High', color: 'var(--wa-color-system-orange)', icon: ChevronUp, iconName: 'chevron-up' },
  { value: 'default', label: 'Default', color: 'var(--wa-color-system-gray)', icon: Minus, iconName: 'minus' },
  { value: 'low', label: 'Low', color: 'var(--wa-color-brand-fill-loud)', icon: ChevronDown, iconName: 'chevron-down' },
];
export interface TicketPrioritySelectProps { name: string; value: TicketPriority; label?: string; disabled?: boolean }
export function TicketPrioritySelect({ name, value, label = 'Priority', disabled = false }: TicketPrioritySelectProps) {
  return <Select className="ticket-priority-select" name={name} value={value} label={label} choices={PRIORITIES} disabled={disabled} />;
}
