import { StatusBadge, type TicketStatus } from './status-badge';
import { TagChip } from './tag-chip';
import { resolveCategoryColor, resolveCategoryIcon } from './category-presentation';
import { LucideIcon } from './lucide-icon';
import { Star } from 'lucide';

export type TicketPriority = 'low' | 'default' | 'high' | 'urgent';

export interface TicketRowProps {
  slug: string;
  title: string;
  status: TicketStatus;
  priority: TicketPriority;
  category: string;
  tags: string[];
  upNext?: boolean;
  blocked?: boolean;
  needsReview?: boolean;
  selected?: boolean;
  busy?: boolean;
  categoryIcon?: string;
  categoryColor?: string;
}

export type TicketRowIndicator = 'needs-review' | 'blocked' | 'up-next' | undefined;

export function ticketRowIndicator(props: Pick<TicketRowProps, 'needsReview' | 'blocked' | 'upNext'>): TicketRowIndicator {
  if (props.needsReview) return 'needs-review';
  if (props.blocked) return 'blocked';
  if (props.upNext) return 'up-next';
  return undefined;
}

export function normalizeTicketRowProps(props: TicketRowProps): TicketRowProps {
  return {
    ...props,
    slug: props.slug.trim() || 'HS2-UNKNOWN',
    title: props.title.trim() || 'Untitled ticket',
    category: props.category.trim() || 'issue',
    tags: props.tags.map(tag => tag.trim()).filter(Boolean),
    upNext: props.upNext ?? false,
    blocked: props.blocked ?? false,
    needsReview: props.needsReview ?? false,
    selected: props.selected ?? false,
    busy: props.busy ?? false,
  };
}

export function TicketRow(raw: TicketRowProps) {
  const props = normalizeTicketRowProps(raw);
  const indicator = ticketRowIndicator(props);
  const categoryIcon = resolveCategoryIcon(props.categoryIcon);
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
      {indicator && <span class={`ticket-list-row__indicator ticket-list-row__indicator--${indicator}`} aria-label={indicator.replace('-', ' ')}></span>}
      <div class="ticket-list-row__body">
        <div class="ticket-list-row__heading">
          {categoryIcon && <span class="ticket-list-row__category" style={`color: ${resolveCategoryColor(props.categoryColor)}`} aria-label={`${props.category} category`}><LucideIcon icon={categoryIcon} name={props.categoryIcon!} class="ticket-list-row__category-icon" /></span>}
          <strong title={props.title}>{props.title}</strong>
          {props.busy && <span class="ticket-list-row__busy" aria-label="AI working">AI working</span>}
        </div>
        <div class="ticket-list-row__metadata">
          <span class="ticket-list-row__slug">{props.slug}</span>
          {!categoryIcon && <span>{props.category}</span>}
          <span class="ticket-list-row__priority-label">{props.priority} priority</span>
          <StatusBadge status={props.status} />
          {props.upNext && <span class="ticket-list-row__up-next" aria-label="Up Next" title="Up Next"><LucideIcon icon={Star} name="star" class="ticket-list-row__up-next-icon" /></span>}
        </div>
        {props.tags.length > 0 && <div class="ticket-list-row__tags">{props.tags.map((tag, index) => TagChip({ id: `row-tag-${index}`, label: tag }))}</div>}
      </div>
    </article>
  );
}
