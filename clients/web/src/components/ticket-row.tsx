import { StatusBadge, type TicketStatus } from './status-badge';
import { TagChip } from './tag-chip';

export type TicketPriority = 'low' | 'default' | 'high' | 'urgent';

export interface TicketRowProps {
  slug: string;
  title: string;
  status: TicketStatus;
  priority: TicketPriority;
  category: string;
  tags: string[];
  upNext?: boolean;
  selected?: boolean;
  busy?: boolean;
}

export function normalizeTicketRowProps(props: TicketRowProps): TicketRowProps {
  return {
    ...props,
    slug: props.slug.trim() || 'HS2-UNKNOWN',
    title: props.title.trim() || 'Untitled ticket',
    category: props.category.trim() || 'issue',
    tags: props.tags.map(tag => tag.trim()).filter(Boolean),
    upNext: props.upNext ?? false,
    selected: props.selected ?? false,
    busy: props.busy ?? false,
  };
}

export function TicketRow(raw: TicketRowProps) {
  const props = normalizeTicketRowProps(raw);
  return (
    <article
      class={`ticket-list-row${props.selected ? ' ticket-list-row--selected' : ''}`}
      data-component="ticket-list-row"
      data-selected={String(props.selected)}
      data-busy={String(props.busy)}
      data-action="select-ticket-row"
      aria-label={`${props.slug}: ${props.title}`}
      aria-selected={String(props.selected)}
      role="option"
      tabindex="0"
    >
      <span class={`ticket-list-row__priority ticket-list-row__priority--${props.priority}`} aria-label={`${props.priority} priority`}></span>
      <div class="ticket-list-row__body">
        <div class="ticket-list-row__heading">
          <strong>{props.title}</strong>
          {props.busy && <span class="ticket-list-row__busy" aria-label="AI working">AI working</span>}
        </div>
        <div class="ticket-list-row__metadata">
          <span class="ticket-list-row__slug">{props.slug}</span>
          <span>{props.category}</span>
          <StatusBadge status={props.status} />
          {props.upNext && <span class="ticket-list-row__up-next">Up Next</span>}
        </div>
        {props.tags.length > 0 && <div class="ticket-list-row__tags">{props.tags.map((tag, index) => TagChip({ id: `row-tag-${index}`, label: tag }))}</div>}
      </div>
    </article>
  );
}
