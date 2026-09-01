import './ticket-status-menu.css';

import { Select, type SelectChoice } from './select';
import { StatusBadge, statusPresentation, type TicketStatus } from './status-badge';

const STATUSES: readonly TicketStatus[] = ['not_started', 'started', 'completed', 'verified', 'backlog'];
const STATUS_CHOICES: readonly SelectChoice<TicketStatus>[] = STATUSES.map(value => ({ value, ...statusPresentation(value) }));
export function TicketStatusMenu({ value }: { value: TicketStatus }) {
  const selected = statusPresentation(value);
  return <Select className="ticket-status-menu" name="inspector-status" ariaLabel={`Change status, ${selected.label}`} value={value} choices={STATUS_CHOICES} renderSelected={choice => <StatusBadge status={choice.value} />} />;
}
