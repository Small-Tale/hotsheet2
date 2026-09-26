import { type CssForegroundColor, foregroundColorVar } from '@kerfjs/ui/css-values';
import { Select } from '@kerfjs/ui/select';
import { ChevronDown, ChevronsUp, ChevronUp, type IconNode, Minus } from 'lucide';

import type { TicketPriority } from './ticket-row';

const PRIORITIES: readonly {
  value: TicketPriority;
  label: string;
  color: CssForegroundColor;
  icon: IconNode;
  iconName: string;
}[] = [
  {
    value: 'urgent',
    label: 'Urgent',
    color: foregroundColorVar('--hs-priority-urgent'),
    icon: ChevronsUp,
    iconName: 'chevrons-up',
  },
  {
    value: 'high',
    label: 'High',
    color: foregroundColorVar('--hs-priority-high'),
    icon: ChevronUp,
    iconName: 'chevron-up',
  },
  {
    value: 'default',
    label: 'Default',
    color: foregroundColorVar('--hs-priority-default'),
    icon: Minus,
    iconName: 'minus',
  },
  {
    value: 'low',
    label: 'Low',
    color: foregroundColorVar('--hs-priority-low'),
    icon: ChevronDown,
    iconName: 'chevron-down',
  },
];
export interface TicketPrioritySelectProps {
  name: string;
  value: TicketPriority;
  label?: string;
  ariaLabel?: string;
  disabled?: boolean;
  placeholder?: boolean;
}
export function TicketPrioritySelect({
  name,
  value,
  label = 'Priority',
  ariaLabel,
  disabled = false,
  placeholder = false,
}: TicketPrioritySelectProps) {
  const accessibleName = ariaLabel ? { ariaLabel } : { label };
  return (
    <Select
      className="ticket-priority-select"
      name={name}
      value={value}
      {...accessibleName}
      choices={PRIORITIES}
      disabled={disabled}
      placeholder={placeholder}
    />
  );
}
