import { colorVar, type CssColor } from '@kerfjs/ui/css-values';
import { Select } from '@kerfjs/ui/select';
import { ChevronDown, ChevronsUp, ChevronUp, type IconNode, Minus } from 'lucide';

import type { TicketPriority } from './ticket-row';

const PRIORITIES: readonly {
  value: TicketPriority;
  label: string;
  color: CssColor;
  icon: IconNode;
  iconName: string;
}[] = [
  {
    value: 'urgent',
    label: 'Urgent',
    color: colorVar('--wa-color-danger-fill-loud'),
    icon: ChevronsUp,
    iconName: 'chevrons-up',
  },
  { value: 'high', label: 'High', color: colorVar('--hs-priority-high'), icon: ChevronUp, iconName: 'chevron-up' },
  { value: 'default', label: 'Default', color: colorVar('--hs-priority-default'), icon: Minus, iconName: 'minus' },
  {
    value: 'low',
    label: 'Low',
    color: colorVar('--wa-color-brand-fill-loud'),
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
