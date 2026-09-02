import './ticket-status-menu.css';

import { Select, type SelectChoice } from './select';
import { StatusBadge, statusPresentation, type TicketStatus } from './status-badge';

export const TICKET_STATUS_CHOICES: readonly SelectChoice<TicketStatus>[] = (['not_started', 'started', 'completed', 'verified', 'backlog', 'archive'] as const)
  .map(value => ({ value, ...statusPresentation(value), separatorBefore: value === 'backlog' }));
export function TicketStatusMenu({ value }: { value: TicketStatus }) {
  const selected = statusPresentation(value);
  return <Select className="ticket-status-menu" name="inspector-status" ariaLabel={`Change status, ${selected.label}`} value={value} choices={TICKET_STATUS_CHOICES} renderSelected={choice => <StatusBadge status={choice.value} />} />;
}
