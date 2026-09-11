import './ticket-status-menu.css';

import { Select, type SelectChoice } from './select';
import { StatusBadge, statusPresentation, type TicketStatus } from './status-badge';

export const TICKET_STATUS_CHOICES: readonly SelectChoice<TicketStatus>[] = (['not_started', 'started', 'completed', 'verified', 'backlog', 'archive'] as const)
  .map(value => ({ value, ...statusPresentation(value), separatorBefore: value === 'backlog' }));
export function TicketStatusMenu({ value, disabled = false }: { value: TicketStatus; disabled?: boolean }) {
  const selected = statusPresentation(value);
  return <Select className="ticket-status-menu" name="inspector-status" ariaLabel={`${disabled ? 'Status' : 'Change status'}, ${selected.label}`} value={value} choices={TICKET_STATUS_CHOICES} disabled={disabled} renderSelected={choice => <StatusBadge status={choice.value} />} />;
}
