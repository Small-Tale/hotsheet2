import './status-badge.css';

import { Badge } from '@kerfjs/ui/badge';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Row } from '@kerfjs/ui/row';
import { Text } from '@kerfjs/ui/text';
import { Archive, BadgeCheck, Circle, CircleCheck, Clock, Clock3, type IconNode, Trash2 } from 'lucide';

import type { StartedPhase } from '../api';

export type TicketStatus = 'not_started' | 'started' | 'completed' | 'verified' | 'backlog' | 'archive' | 'deleted';
export type StatusBadgeAppearance = 'filled' | 'plain';
export const STARTED_PHASE_LABELS: Record<StartedPhase, string> = {
  analyzing: 'Analyzing',
  planning: 'Planning',
  working: 'Working',
  initial_testing: 'Initial testing',
  integrating: 'Integrating',
  final_testing: 'Final testing',
};

export interface StatusBadgeProps {
  status: TicketStatus;
  startedPhase?: StartedPhase;
  showIcon?: boolean;
  appearance?: StatusBadgeAppearance;
  compact?: boolean;
  interactive?: boolean;
  actionLabel?: string;
  action?: string;
  hasPopup?: 'menu';
  slot?: string;
}

const presentation: Record<TicketStatus, { icon: IconNode; iconName: string; label: string }> = {
  not_started: { icon: Circle, iconName: 'circle', label: 'Not started' },
  started: { icon: Clock, iconName: 'clock', label: 'Started' },
  completed: { icon: CircleCheck, iconName: 'circle-check', label: 'Completed' },
  verified: { icon: BadgeCheck, iconName: 'badge-check', label: 'Verified' },
  backlog: { icon: Clock3, iconName: 'clock-3', label: 'Backlog' },
  archive: { icon: Archive, iconName: 'archive', label: 'Archive' },
  deleted: { icon: Trash2, iconName: 'trash-2', label: 'Deleted' },
};

export function statusPresentation(status: TicketStatus) {
  return presentation[status];
}

/** Each lifecycle status's semantic Kerf Badge tone. */
export const STATUS_BADGE_TONES = {
  not_started: 'neutral',
  started: 'warning',
  completed: 'success',
  verified: 'info',
  backlog: 'neutral',
  archive: 'neutral',
  deleted: 'neutral',
} as const satisfies Record<TicketStatus, string>;

/**
 * A ticket's lifecycle status (and Started phase) as text plus a reinforcing Lucide icon. Kerf's
 * `Badge` owns the filled pill, its status tone, and the compact size; `plain` is the bare icon and
 * quiet label (HS2-3FMK0T). The interactive form is the TicketStatusMenu trigger button.
 */
export function StatusBadge({
  status,
  startedPhase,
  showIcon = true,
  appearance = 'filled',
  compact = false,
  interactive = false,
  actionLabel,
  action,
  hasPopup,
  slot,
}: StatusBadgeProps) {
  const value = statusPresentation(status);
  const phaseLabel = startedPhase && status === 'started' ? STARTED_PHASE_LABELS[startedPhase] : undefined;
  const label = phaseLabel ?? value.label;
  const row = (
    <Row gap="2xs" vAlign="middle">
      {showIcon && <LucideIcon icon={value.icon} name={value.iconName} size={compact ? 'xs' : 's'} />}
      {appearance === 'plain' ? (
        <Text variant="span" tone="quiet" size="compact">
          {label}
        </Text>
      ) : (
        <span>{label}</span>
      )}
    </Row>
  );
  const content =
    appearance === 'plain' ? (
      row
    ) : (
      <Badge tone={STATUS_BADGE_TONES[status]} size={compact ? 'compact' : 'default'}>
        {row}
      </Badge>
    );
  const identity = {
    'data-component': 'status-badge',
    'data-status': status,
    'data-appearance': appearance,
    'data-compact': compact ? 'true' : undefined,
  };
  return interactive ? (
    <button
      type="button"
      slot={slot}
      class="status-badge"
      {...identity}
      data-interactive="true"
      aria-label={actionLabel}
      data-action={action}
      aria-haspopup={hasPopup}
    >
      {content}
    </button>
  ) : (
    <span slot={slot} class="status-badge" {...identity}>
      {content}
    </span>
  );
}

/**
 * The solid neutral "Blocked" marker beside a ticket's status. Kerf's `Badge` is the root; its
 * identity is the `data-blocked-badge` root attribute (HS2-PQHN1J), so there is no wrapper.
 */
export function BlockedBadge({ compact = false }: { compact?: boolean }) {
  return (
    <Badge appearance="solid" size={compact ? 'compact' : 'default'} rootAttributes={{ 'data-blocked-badge': 'true' }}>
      Blocked
    </Badge>
  );
}
