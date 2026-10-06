import './ticket-row.css';

import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { ChevronDown, ChevronsUp, ChevronUp, CircleAlert, type IconNode, Minus, Star } from 'lucide';

import type { ClaimEtaPresentation } from '../active-ticket-work';
import type { StartedPhase } from '../api';
import { SEARCH_AND_COMPOSER_ACTIONS } from '../interaction-attrs/search-and-composer';
import { TICKET_SELECTION_ACTIONS } from '../interaction-attrs/ticket-selection';
import { ActiveClaimSpinner, ClaimEta } from './active-claim';
import {
  categoryAbbreviation,
  defaultCategoryPresentation,
  resolveCategoryIcon,
  resolveCategoryIconColor,
} from './category-presentation';
import { ConfidenceBadge } from './confidence-badge';
import { BlockedBadge, StatusBadge, type TicketStatus } from './status-badge';
import { TagChip } from './tag-chip';

export type TicketPriority = 'low' | 'default' | 'high' | 'urgent';
export type TicketRowPresentation = 'list' | 'column';
/**
 * Where a list row sits against its rounded, clipping list container: its matching corners round so the
 * row's hover and selection border follows the container's curve (HS2-4APEJP).
 */
export type TicketRowListEdge = 'start' | 'end' | 'only';

export interface TicketRowProps {
  slug: string;
  title: string;
  status: TicketStatus;
  startedPhase?: StartedPhase;
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
  /** Progress toward the live claim's ETA, shown beside the active-work indicator (HS2-XQMDQB). */
  claimEta?: ClaimEtaPresentation;
  updatedLabel?: string;
  /**
   * Derived AI completion confidence (0-100) of a completed/verified ticket (HS2-A0Q6G6).
   * Shown as the compact pill among the row's other pills; ignored for any other status.
   */
  latestConfidence?: number;
  cutPending?: boolean;
  presentation?: TicketRowPresentation;
  listEdge?: TicketRowListEdge;
}

export type TicketRowIndicator = 'needs-review' | 'blocked' | 'up-next' | undefined;

const priorityPresentation: Record<TicketPriority, { icon: IconNode; name: string; color: string }> = {
  urgent: { icon: ChevronsUp, name: 'chevrons-up', color: 'var(--hs-priority-urgent)' },
  high: { icon: ChevronUp, name: 'chevron-up', color: 'var(--hs-priority-high)' },
  default: { icon: Minus, name: 'minus', color: 'var(--hs-priority-default)' },
  low: { icon: ChevronDown, name: 'chevron-down', color: 'var(--hs-priority-low)' },
};

/** The confidence a summary shows: only a completed/verified ticket carries one. */
export function ticketRowConfidence(props: Pick<TicketRowProps, 'status' | 'latestConfidence'>): number | undefined {
  return props.status === 'completed' || props.status === 'verified' ? props.latestConfidence : undefined;
}

export function getPriorityPresentation(priority: TicketPriority) {
  return priorityPresentation[priority];
}

export function ticketRowIndicator(
  props: Pick<TicketRowProps, 'feedbackNeeded' | 'needsReview' | 'blocked' | 'upNext'>,
): TicketRowIndicator {
  if (props.feedbackNeeded || props.needsReview) return 'needs-review';
  if (props.blocked) return 'blocked';
  if (props.upNext) return 'up-next';
  return undefined;
}

function ActiveClaimIndicator({ agentName = 'AI' }: { agentName?: string }) {
  const label = `${agentName} is actively working on this ticket`;
  return (
    <span class="ticket-list-row__claim" title={`${label} while it stays active`}>
      <ActiveClaimSpinner label={label} />
    </span>
  );
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
    tags: props.tags.map((tag) => tag.trim()).filter(Boolean),
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
  const confidence = ticketRowConfidence(props);
  const category = categoryIcon ? (
    <span
      class="ticket-list-row__category"
      style={`color: ${resolveCategoryIconColor(props.categoryColor)}`}
      aria-label={`${props.category} category`}
    >
      <span class="ticket-list-row__category-icon">
        <LucideIcon
          icon={categoryIcon}
          name={props.categoryIcon!}
          size={props.presentation === 'column' ? 15.2 : 26.4}
        />
      </span>
    </span>
  ) : (
    <span
      class="ticket-list-row__category ticket-list-row__category--label"
      style={`color: ${resolveCategoryIconColor(props.categoryColor)}`}
      aria-label={`${props.category} category`}
      title={props.category}
    >
      {categoryAbbreviation(props.category, props.categoryShortLabel)}
    </span>
  );
  return (
    <div class="ticket-list-row-container" data-key={`ticket:${props.slug}`} data-component="ticket-list-row-container">
      <article
        class={`ticket-list-row ticket-list-row--${props.presentation}${props.selected ? ' ticket-list-row--selected' : ''}`}
        data-component="ticket-list-row"
        data-presentation={props.presentation}
        data-list-edge={props.listEdge}
        data-status={props.status}
        data-ticket-slug={props.slug}
        data-attachment-drop-target="true"
        data-selected={String(props.selected)}
        data-busy={String(props.busy)}
        data-cut-pending={String(Boolean(props.cutPending))}
        {...TICKET_SELECTION_ACTIONS.selectTicketRow.attrs}
        aria-label={`${props.slug}: ${props.title}`}
        aria-selected={String(props.selected)}
        role="option"
        tabindex="0"
        draggable="true"
      >
        {indicator && (
          <span
            class={`ticket-list-row__indicator ticket-list-row__indicator--${indicator}`}
            aria-label={indicator.replace('-', ' ')}
          />
        )}
        <div class="ticket-list-row__body">
          {props.presentation === 'list' && category}
          <div class="ticket-list-row__content">
            <div class="ticket-list-row__first-line">
              <div class="ticket-list-row__identity">
                <span class="ticket-list-row__updated">{props.updatedLabel}</span>
                {props.presentation === 'column' && category}
                <span class="ticket-list-row__slug">{props.slug}</span>
                <span
                  class="ticket-list-row__priority"
                  style={`color: ${priority.color}`}
                  aria-label={`${props.priority} priority`}
                  title={`${props.priority} priority`}
                >
                  <span class="ticket-list-row__priority-icon">
                    <LucideIcon icon={priority.icon} name={priority.name} size="s" />
                  </span>
                </span>
                <strong title={props.title}>{props.title}</strong>
              </div>
            </div>
            <div class="ticket-list-row__metadata">
              {props.upNextEligible && (
                <button
                  type="button"
                  class={`ticket-list-row__up-next${props.upNext ? ' ticket-list-row__up-next--active' : ''}`}
                  {...SEARCH_AND_COMPOSER_ACTIONS.toggleRowUpNext.attrs}
                  aria-label={props.upNext ? 'Remove from Up Next' : 'Add to Up Next'}
                  title={props.upNext ? 'Remove from Up Next' : 'Add to Up Next'}
                >
                  <span class="ticket-list-row__up-next-icon">
                    <LucideIcon icon={Star} name="star" size="s" appearance={props.upNext ? 'solid' : 'outline'} />
                  </span>
                </button>
              )}
              {props.presentation === 'list' && (
                <StatusBadge status={props.status} startedPhase={props.startedPhase} compact />
              )}
              {props.busy && <ActiveClaimIndicator agentName={props.agentName} />}
              {needsReview && (
                <span class="ticket-list-row__feedback" aria-label="Needs review" title="Needs review">
                  <span class="ticket-list-row__feedback-icon">
                    <LucideIcon icon={CircleAlert} name="circle-alert" size={12.8} />
                  </span>
                  Needs review
                </span>
              )}
              {props.blocked && <BlockedBadge compact />}
              {props.busy && (
                <span class="ticket-list-row__owner" aria-label={props.agentName}>
                  {props.agentName}
                </span>
              )}
              {props.busy && props.claimEta && <ClaimEta eta={props.claimEta} />}
              {confidence !== undefined && <ConfidenceBadge value={confidence} />}
              {props.tags.length > 0 && (
                <div class="ticket-list-row__tags">
                  {props.tags.map((tag, index) => TagChip({ id: `row-tag-${index}`, label: tag }))}
                </div>
              )}
            </div>
          </div>
        </div>
      </article>
    </div>
  );
}
