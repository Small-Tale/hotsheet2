import '@awesome.me/webawesome/dist/components/button/button.js';
import '@awesome.me/webawesome/dist/components/dropdown/dropdown.js';
import '@awesome.me/webawesome/dist/components/dropdown-item/dropdown-item.js';
import { StatusBadge, statusPresentation, type TicketStatus } from './status-badge';
import { LucideIcon } from './lucide-icon';
import './ticket-status-menu.css';

const STATUSES: readonly TicketStatus[] = ['not_started', 'started', 'completed', 'verified', 'backlog'];
export function TicketStatusMenu({ value }: { value: TicketStatus }) {
  const selected = statusPresentation(value);
  return <wa-dropdown class="ticket-status-menu" placement="bottom-start">
    <wa-button slot="trigger" appearance="plain" with-caret aria-label={`Change status, ${selected.label}`}><StatusBadge status={value} /></wa-button>
    {STATUSES.map(status => {
      const option = statusPresentation(status);
      return <wa-dropdown-item type="checkbox" checked={status === value} data-inspector-status={status} value={status}><span slot="start" class="ticket-status-menu__icon"><LucideIcon icon={option.icon} name={option.iconName} /></span>{option.label}</wa-dropdown-item>;
    })}
  </wa-dropdown>;
}
