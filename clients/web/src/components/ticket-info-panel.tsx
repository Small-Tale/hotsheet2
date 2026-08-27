import '@awesome.me/webawesome/dist/components/button/button.js';
import { BookOpen } from 'lucide';
import { LucideIcon } from './lucide-icon';
import { StatusBadge, type TicketStatus } from './status-badge';
import { TagChip } from './tag-chip';
import { TicketCategorySelect } from './ticket-category-select';
import { TicketPrioritySelect } from './ticket-priority-select';
import type { TicketPriority } from './ticket-row';
import './ticket-inspector-panel.css';

export interface TicketInfoPanelProps { status: TicketStatus; priority: TicketPriority; category: string; tags: string[]; details: string }
export function TicketInfoPanel({ status, priority, category, tags, details }: TicketInfoPanelProps) {
  return <div class="ticket-inspector__content" data-component="ticket-info-panel">
    <section class="ticket-inspector__metadata" aria-label="Ticket metadata">
      <TicketCategorySelect name="inspector-category" value={category} />
      <TicketPrioritySelect name="inspector-priority" value={priority} />
      <div><span>Status</span><StatusBadge status={status} /></div>
    </section>
    <section><header><h2>Details</h2><wa-button appearance="plain" data-action="open-ticket-reader" aria-label="Open ticket reader"><LucideIcon icon={BookOpen} name="book-open" /></wa-button></header><p>{details}</p></section>
    <section><header><h2>Tags</h2></header><div class="ticket-inspector__tags">{tags.map((tag, index) => TagChip({ id: `inspector-tag-${index}`, label: tag }))}</div></section>
  </div>;
}
