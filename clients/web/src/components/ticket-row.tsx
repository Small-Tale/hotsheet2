// The shared row shell loads before the TicketRow block so its column presentation can override it.
import './ticket-row-shell.css';
import './ticket-row.css';

import { Badge } from '@kerfjs/ui/badge';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Row } from '@kerfjs/ui/row';
import { CircleAlert, Star } from 'lucide';

import type { ClaimEtaPresentation } from '../active-ticket-work';
import type { StartedPhase } from '../api';
import { SEARCH_AND_COMPOSER_ACTIONS } from '../interaction-attrs/search-and-composer';
import { TICKET_SELECTION_ACTIONS } from '../interaction-attrs/ticket-selection';
import { ticketDisplayNumber } from '../ticket-display-number';
import { ActiveClaimIndicator, ClaimEta } from './active-claim';
import {
  categoryAbbreviation,
  defaultCategoryPresentation,
  resolveCategoryIcon,
  resolveCategoryIconColor,
} from './category-presentation';
import { ConfidenceBadge } from './confidence-badge';
import { priorityPresentation, type TicketPriority } from './priority-presentation';
import { BlockedBadge, StatusBadge, type TicketStatus } from './status-badge';
import { TagChip } from './tag-chip';
import { TicketSourceIcon, type TicketSourceIdentity } from './ticket-source-icon';

export type { TicketPriority } from './priority-presentation';
export type TicketRowPresentation = 'list' | 'column';
/**
 * Where a list row sits against its rounded, clipping list container: its matching corners round so the
 * row's hover and selection border follows the container's curve (HS2-4APEJP).
 */
export type TicketRowListEdge = 'start' | 'end' | 'only';

/**
 * The shared selectable row shell (HS2-1FH8BG): surface, divider, list-edge rounding, hover, and
 * selection. TicketRow renders it; the diagnostic CorruptTicketRow composes it with the `danger`
 * tone (a danger rail and a selection wiggle) instead of rebuilding the shell.
 */
export const TICKET_ROW_SHELL_CLASS = 'ticket-row-shell';
export type TicketRowShellTone = 'danger';
export function ticketRowShellAttributes({
  selected,
  listEdge,
  tone,
}: {
  selected: boolean;
  listEdge?: TicketRowListEdge;
  tone?: TicketRowShellTone;
}) {
  return { 'data-selected': String(selected), 'data-list-edge': listEdge, 'data-tone': tone };
}

export interface TicketRowProps {
  slug: string;
  title: string;
  status: TicketStatus;
  startedPhase?: StartedPhase;
  priority: TicketPriority;
  category: string;
  source?: TicketSourceIdentity;
  /** The provider mark helps distinguish cards only when this project has multiple ticket sources. */
  showSourceIcon?: boolean;
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
  pendingOperationCount?: number;
  pendingSyncState?: 'queued' | 'sending' | 'rate_limited' | 'needs_attention';
  cutPending?: boolean;
  presentation?: TicketRowPresentation;
  listEdge?: TicketRowListEdge;
}

export type TicketRowIndicator = 'needs-review' | 'blocked' | 'up-next' | undefined;

/** The confidence a summary shows: only a completed/verified ticket carries one. */
export function ticketRowConfidence(props: Pick<TicketRowProps, 'status' | 'latestConfidence'>): number | undefined {
  return props.status === 'completed' || props.status === 'verified' ? props.latestConfidence : undefined;
}

/** The row's priority indicator reads the shared priority presentation (HS2-8MT5KG). */
export function getPriorityPresentation(priority: TicketPriority) {
  const { icon, iconName, colorToken } = priorityPresentation(priority);
  return { icon, name: iconName, color: `var(${colorToken})` };
}

export function ticketRowIndicator(
  props: Pick<TicketRowProps, 'feedbackNeeded' | 'needsReview' | 'blocked' | 'upNext'>,
): TicketRowIndicator {
  if (props.feedbackNeeded || props.needsReview) return 'needs-review';
  if (props.blocked) return 'blocked';
  if (props.upNext) return 'up-next';
  return undefined;
}

function RowClaimIndicator({ agentName = 'AI', claimEta }: { agentName?: string; claimEta?: ClaimEtaPresentation }) {
  const label = `${agentName} is actively working on this ticket`;
  return (
    <span class="ticket-list-row__claim" title={`${label} while it stays active`}>
      <ActiveClaimIndicator label={label} eta={claimEta} />
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
  const cardSource = props.showSourceIcon !== false && props.source;
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
        class={`${TICKET_ROW_SHELL_CLASS} ticket-list-row ticket-list-row--${props.presentation}${props.selected ? ' ticket-list-row--selected' : ''}`}
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
        <div class="ticket-list-row__body" data-has-source={String(Boolean(cardSource))}>
          {props.presentation === 'list' && category}
          {props.presentation === 'list' && cardSource && <TicketSourceIcon source={cardSource} />}
          <div class="ticket-list-row__content">
            <div class="ticket-list-row__first-line">
              <div class="ticket-list-row__identity">
                <span class="ticket-list-row__updated">{props.updatedLabel}</span>
                {props.presentation === 'column' ? (
                  <span class="ticket-list-row__identifier">
                    {category}
                    {cardSource && (
                      <span class="ticket-list-row__source">
                        <TicketSourceIcon source={cardSource} size="compact" />
                      </span>
                    )}
                    <span class="ticket-list-row__slug">{ticketDisplayNumber(props.slug, props.source?.provider)}</span>
                  </span>
                ) : (
                  <span class="ticket-list-row__slug">{ticketDisplayNumber(props.slug, props.source?.provider)}</span>
                )}
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
                  <LucideIcon icon={Star} name="star" size="s" appearance={props.upNext ? 'solid' : 'outline'} />
                </button>
              )}
              {(props.presentation === 'list' ||
                (props.presentation === 'column' && props.status === 'started' && props.startedPhase)) && (
                <StatusBadge status={props.status} startedPhase={props.startedPhase} compact />
              )}
              {props.busy && <RowClaimIndicator agentName={props.agentName} claimEta={props.claimEta} />}
              {needsReview && (
                <span class="ticket-list-row__feedback" role="img" aria-label="Needs review" title="Needs review">
                  <Badge tone="warning" appearance="outline" size="compact" ariaHidden>
                    <Row gap="2xs" vAlign="middle">
                      <LucideIcon icon={CircleAlert} name="circle-alert" size={12.8} />
                      <span>Needs review</span>
                    </Row>
                  </Badge>
                </span>
              )}
              {props.blocked && <BlockedBadge compact />}
              {Boolean(props.pendingOperationCount) && (
                <span
                  class="ticket-list-row__pending-sync"
                  data-state={props.pendingSyncState ?? 'queued'}
                  title={`${props.pendingOperationCount} local edit${props.pendingOperationCount === 1 ? '' : 's'} awaiting Jira confirmation`}
                >
                  <Badge tone={props.pendingSyncState === 'needs_attention' ? 'danger' : 'warning'} size="compact">
                    {`${
                      props.pendingSyncState === 'needs_attention'
                        ? 'Sync needs attention'
                        : props.pendingSyncState === 'sending'
                          ? 'Sending…'
                          : props.pendingSyncState === 'rate_limited'
                            ? 'Waiting for Jira'
                            : 'Pending sync'
                    }${props.pendingOperationCount! > 1 ? ` (${props.pendingOperationCount})` : ''}`}
                  </Badge>
                </span>
              )}
              {props.busy && (
                <span class="ticket-list-row__owner" aria-label={props.agentName} title={props.agentName}>
                  {props.agentName}
                </span>
              )}
              {props.busy && props.claimEta && <ClaimEta eta={props.claimEta} />}
              {confidence !== undefined && <ConfidenceBadge value={confidence} />}
              {props.tags.length > 0 && (
                <Row wrap gap="xs" vAlign="middle" rootAttributes={{ 'data-row-tags': 'true' }}>
                  {props.tags.map((tag, index) => TagChip({ id: `row-tag-${index}`, label: tag }))}
                </Row>
              )}
            </div>
          </div>
        </div>
      </article>
    </div>
  );
}
