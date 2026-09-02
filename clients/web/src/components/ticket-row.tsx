import './ticket-row.css';

import { ChevronDown, ChevronsUp, ChevronUp, CircleAlert, type IconNode,Minus, Star } from 'lucide';

import { categoryAbbreviation, defaultCategoryPresentation, resolveCategoryIcon, resolveCategoryIconColor } from './category-presentation';
import { LucideIcon } from './lucide-icon';
import { BlockedBadge, StatusBadge, type TicketStatus } from './status-badge';
import { TagChip } from './tag-chip';

export type TicketPriority = 'low' | 'default' | 'high' | 'urgent';
export type TicketRowPresentation = 'list' | 'column';

export interface TicketRowProps {
  slug: string;
  title: string;
  status: TicketStatus;
  priority: TicketPriority;
  category: string;
  tags: string[];
  upNext?: boolean;
  upNextEligible?: boolean;
  blocked?: boolean;
  needsReview?: boolean;
  /** The ticket has an unresolved `feedback_needed` note — it is waiting on the user. */
  feedbackNeeded?: boolean;
  selected?: boolean;
  busy?: boolean;
  categoryIcon?: string;
  categoryColor?: string;
  categoryShortLabel?: string;
  agentName?: string;
  updatedLabel?: string;
  cutPending?: boolean;
  presentation?: TicketRowPresentation;
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

export function ticketRowIndicator(props: Pick<TicketRowProps, 'feedbackNeeded' | 'needsReview' | 'blocked' | 'upNext'>): TicketRowIndicator {
  if (props.feedbackNeeded || props.needsReview) return 'needs-review';
  if (props.blocked) return 'blocked';
  if (props.upNext) return 'up-next';
  return undefined;
}

export function normalizeTicketRowProps(props: TicketRowProps): TicketRowProps {
  const category = props.category.trim() || 'issue';
  const categoryPresentation = defaultCategoryPresentation(category);
  return {
    ...props,
    slug: props.slug.trim() || 'HS2-UNKNOWN',
    title: props.title.trim() || 'Untitled ticket',
    category,
    categoryIcon: props.categoryIcon === undefined ? categoryPresentation?.iconName : props.categoryIcon,
    categoryColor: props.categoryColor ?? categoryPresentation?.color,
    tags: props.tags.map(tag => tag.trim()).filter(Boolean),
    upNext: props.upNext ?? false,
    upNextEligible: props.upNextEligible ?? (props.status === 'not_started' || props.status === 'started'),
    blocked: props.blocked ?? false,
    needsReview: props.needsReview ?? false,
    feedbackNeeded: props.feedbackNeeded ?? false,
    selected: props.selected ?? false,
    busy: props.busy ?? false,
    agentName: props.agentName?.trim() || 'AI',
    updatedLabel: props.updatedLabel?.trim() || 'Recently',
    presentation: props.presentation ?? 'list',
  };
}

export function TicketRow(raw: TicketRowProps) {
  const props = normalizeTicketRowProps(raw);
  const needsReview = props.needsReview || props.feedbackNeeded;
  const indicator = ticketRowIndicator(props);
  const categoryIcon = resolveCategoryIcon(props.categoryIcon);
  const priority = getPriorityPresentation(props.priority);
  const category = categoryIcon
    ? <span class="ticket-list-row__category" style={`color: ${resolveCategoryIconColor(props.categoryColor)}`} aria-label={`${props.category} category`}><LucideIcon icon={categoryIcon} name={props.categoryIcon!} class="ticket-list-row__category-icon" /></span>
    : <span class="ticket-list-row__category ticket-list-row__category--label" style={`color: ${resolveCategoryIconColor(props.categoryColor)}`} aria-label={`${props.category} category`} title={props.category}>{categoryAbbreviation(props.category, props.categoryShortLabel)}</span>;
  return (
    <div class="ticket-list-row-container" data-component="ticket-list-row-container">
      <article
        class={`ticket-list-row ticket-list-row--${props.presentation}${props.selected ? ' ticket-list-row--selected' : ''}`}
        data-component="ticket-list-row"
        data-presentation={props.presentation}
        data-status={props.status}
        data-ticket-slug={props.slug}
        data-attachment-drop-target="true"
        data-selected={String(props.selected)}
        data-busy={String(props.busy)}
        data-cut-pending={String(Boolean(props.cutPending))}
        data-action="select-ticket-row"
        aria-label={`${props.slug}: ${props.title}`}
        aria-selected={String(props.selected)}
        role="option"
        tabindex="0"
        draggable="true"
      >
        {indicator && <span class={`ticket-list-row__indicator ticket-list-row__indicator--${indicator}`} aria-label={indicator.replace('-', ' ')}></span>}
        <div class="ticket-list-row__body">
          {props.presentation === 'list' && category}
          <div class="ticket-list-row__content">
            <div class="ticket-list-row__first-line">
              <div class="ticket-list-row__identity">
                <span class="ticket-list-row__updated">{props.updatedLabel}</span>
                {props.presentation === 'column' && category}
                <span class="ticket-list-row__slug">{props.slug}</span>
                <span class="ticket-list-row__priority" style={`color: ${priority.color}`} aria-label={`${props.priority} priority`} title={`${props.priority} priority`}><LucideIcon icon={priority.icon} name={priority.name} class="ticket-list-row__priority-icon" /></span>
                <strong title={props.title}>{props.title}</strong>
              </div>
            </div>
            <div class="ticket-list-row__metadata">
              {props.upNextEligible && <button type="button" class={`ticket-list-row__up-next${props.upNext ? ' ticket-list-row__up-next--active' : ''}`} data-action="toggle-row-up-next" aria-label={props.upNext ? 'Remove from Up Next' : 'Add to Up Next'} title={props.upNext ? 'Remove from Up Next' : 'Add to Up Next'}><LucideIcon icon={Star} name="star" class="ticket-list-row__up-next-icon" /></button>}
              <StatusBadge status={props.status} compact />
              {props.busy && <span class="ticket-list-row__active-work" role="img" aria-label={`${props.agentName} actively working`} title={`${props.agentName} is actively working on this ticket`}></span>}
              {needsReview && <span class="ticket-list-row__feedback" aria-label="Needs review" title="Needs review"><LucideIcon icon={CircleAlert} name="circle-alert" class="ticket-list-row__feedback-icon" />Needs review</span>}
              {props.blocked && <BlockedBadge compact />}
              <span class="ticket-list-row__owner" aria-label={props.agentName}>{props.agentName}</span>
              {props.tags.length > 0 && <div class="ticket-list-row__tags">{props.tags.map((tag, index) => TagChip({ id: `row-tag-${index}`, label: tag }))}</div>}
            </div>
          </div>
        </div>
      </article>
    </div>
  );
}
