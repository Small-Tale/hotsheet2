import { StatusBadge, type TicketStatus } from './status-badge';
import { TagChip } from './tag-chip';
import { resolveCategoryIcon, resolveCategoryIconColor } from './category-presentation';
import { LucideIcon } from './lucide-icon';
import { ChevronDown, ChevronUp, ChevronsUp, Minus, Star, type IconNode } from 'lucide';

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
  agentName?: string;
  updatedLabel?: string;
}

export type TicketRowIndicator = 'needs-review' | 'blocked' | 'up-next' | undefined;

const priorityPresentation: Record<TicketPriority, { icon: IconNode; name: string; color: string }> = {
  urgent: { icon: ChevronsUp, name: 'chevrons-up', color: '#ef4444' },
  high: { icon: ChevronUp, name: 'chevron-up', color: '#f97316' },
  default: { icon: Minus, name: 'minus', color: '#6b7280' },
  low: { icon: ChevronDown, name: 'chevron-down', color: '#3b82f6' },
};

export function getPriorityPresentation(priority: TicketPriority) {
  return priorityPresentation[priority];
}

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
    agentName: props.agentName?.trim() || 'AI',
    updatedLabel: props.updatedLabel?.trim() || 'Recently',
  };
}

export function TicketRow(raw: TicketRowProps) {
  const props = normalizeTicketRowProps(raw);
  const indicator = ticketRowIndicator(props);
  const categoryIcon = resolveCategoryIcon(props.categoryIcon);
  const priority = getPriorityPresentation(props.priority);
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
        {categoryIcon
          ? <span class="ticket-list-row__category" style={`color: ${resolveCategoryIconColor(props.categoryColor)}`} aria-label={`${props.category} category`}><LucideIcon icon={categoryIcon} name={props.categoryIcon!} class="ticket-list-row__category-icon" /></span>
          : <span class="ticket-list-row__category ticket-list-row__category--label" style={`color: ${resolveCategoryIconColor(props.categoryColor)}`}>{props.category}</span>}
        <div class="ticket-list-row__content">
          <div class="ticket-list-row__first-line">
            <div class="ticket-list-row__identity">
              <span class="ticket-list-row__slug">{props.slug}</span>
              <span class="ticket-list-row__priority" style={`color: ${priority.color}`} aria-label={`${props.priority} priority`} title={`${props.priority} priority`}><LucideIcon icon={priority.icon} name={priority.name} class="ticket-list-row__priority-icon" /></span>
              <strong title={props.title}>{props.title}</strong>
            </div>
            <span class="ticket-list-row__updated">{props.updatedLabel}</span>
          </div>
          <div class="ticket-list-row__metadata">
            <button type="button" class={`ticket-list-row__up-next${props.upNext ? ' ticket-list-row__up-next--active' : ''}`} data-action="toggle-row-up-next" aria-label={props.upNext ? 'Remove from Up Next' : 'Add to Up Next'} title={props.upNext ? 'Remove from Up Next' : 'Add to Up Next'}><LucideIcon icon={Star} name="star" class="ticket-list-row__up-next-icon" /></button>
            <StatusBadge status={props.status} compact />
            <span class={`ticket-list-row__owner${props.busy ? ' ticket-list-row__owner--active' : ''}`} aria-label={props.busy ? `${props.agentName} working` : props.agentName}>{props.agentName}</span>
            {props.tags.length > 0 && <div class="ticket-list-row__tags">{props.tags.map((tag, index) => TagChip({ id: `row-tag-${index}`, label: tag }))}</div>}
          </div>
        </div>
      </div>
    </article>
  );
}
