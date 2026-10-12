import { foregroundColorVar } from '@kerfjs/ui/css-values';
import { Select } from '@kerfjs/ui/select';

import { TICKET_PRIORITIES, type TicketPriority } from './priority-presentation';

/** Kerf Select choices from the shared priority presentation that TicketRow also reads (HS2-8MT5KG). */
const PRIORITY_CHOICES = TICKET_PRIORITIES.map(({ value, label, colorToken, icon, iconName }) => ({
  value,
  label,
  color: foregroundColorVar(colorToken),
  icon,
  iconName,
}));

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
      choices={PRIORITY_CHOICES}
      disabled={disabled}
      placeholder={placeholder}
      triggerWidth="fill"
    />
  );
}
