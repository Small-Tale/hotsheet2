import './status-badge.css';

import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Archive, BadgeCheck, Circle, CircleCheck, Clock, Clock3, type IconNode, Trash2 } from 'lucide';

import type { StartedPhase } from '../api';

export type TicketStatus = 'not_started' | 'started' | 'completed' | 'verified' | 'backlog' | 'archive' | 'deleted';
export type StatusBadgeAppearance = 'filled' | 'plain';
/** Label weight: `semibold` is the quieter trigger label inside the TicketStatusMenu. */
export type StatusBadgeWeight = 'bold' | 'semibold';
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
  weight?: StatusBadgeWeight;
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

export function StatusBadge({
  status,
  startedPhase,
  showIcon = true,
  appearance = 'filled',
  compact = false,
  interactive = false,
  weight = 'bold',
  actionLabel,
  action,
  hasPopup,
  slot,
}: StatusBadgeProps) {
  const value = statusPresentation(status);
  const phaseLabel = startedPhase && status === 'started' ? STARTED_PHASE_LABELS[startedPhase] : undefined;
  const className = `status-badge status-badge--${status} status-badge--${appearance}${compact ? ' status-badge--compact' : ''}${interactive ? ' status-badge--interactive' : ''}${weight === 'semibold' ? ' status-badge--semibold' : ''}`;
  const content = (
    <>
      {showIcon && (
        <span class="status-badge__icon">
          <LucideIcon icon={value.icon} name={value.iconName} size={compact ? 'xs' : 13.6} />
        </span>
      )}
      <span>{phaseLabel ?? value.label}</span>
    </>
  );
  return interactive ? (
    <button
      type="button"
      slot={slot}
      class={className}
      data-component="status-badge"
      data-status={status}
      data-appearance={appearance}
      aria-label={actionLabel}
      data-action={action}
      aria-haspopup={hasPopup}
    >
      {content}
    </button>
  ) : (
    <span slot={slot} class={className} data-component="status-badge" data-status={status} data-appearance={appearance}>
      {content}
    </span>
  );
}

export function BlockedBadge({ compact = false }: { compact?: boolean }) {
  return (
    <span class={`blocked-badge${compact ? ' blocked-badge--compact' : ''}`} data-component="blocked-badge">
      Blocked
    </span>
  );
}
