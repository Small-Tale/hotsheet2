import { Archive, BadgeCheck, Circle, CircleCheck, Clock, type IconNode } from 'lucide';
import { LucideIcon } from './lucide-icon';
import './status-badge.css';

export type TicketStatus = 'not_started' | 'started' | 'completed' | 'verified' | 'backlog';
export type StatusBadgeAppearance = 'filled' | 'plain';

export interface StatusBadgeProps {
  status: TicketStatus;
  showIcon?: boolean;
  appearance?: StatusBadgeAppearance;
  compact?: boolean;
}

const presentation: Record<TicketStatus, { icon: IconNode; iconName: string; label: string }> = {
  not_started: { icon: Circle, iconName: 'circle', label: 'Not started' },
  started: { icon: Clock, iconName: 'clock', label: 'Started' },
  completed: { icon: CircleCheck, iconName: 'circle-check', label: 'Completed' },
  verified: { icon: BadgeCheck, iconName: 'badge-check', label: 'Verified' },
  backlog: { icon: Archive, iconName: 'archive', label: 'Backlog' },
};

export function statusPresentation(status: TicketStatus) {
  return presentation[status];
}

export function StatusBadge({ status, showIcon = true, appearance = 'filled', compact = false }: StatusBadgeProps) {
  const value = statusPresentation(status);
  return (
    <span class={`status-badge status-badge--${status} status-badge--${appearance}${compact ? ' status-badge--compact' : ''}`} data-component="status-badge" data-status={status} data-appearance={appearance}>
      {showIcon && <LucideIcon class="status-badge__icon" icon={value.icon} name={value.iconName} />}
      <span>{value.label}</span>
    </span>
  );
}
