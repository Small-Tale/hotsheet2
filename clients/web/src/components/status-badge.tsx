import './status-badge.css';

import { Archive, BadgeCheck, Circle, CircleCheck, Clock, Clock3, type IconNode } from 'lucide';

import { LucideIcon } from './lucide-icon';

export type TicketStatus = 'not_started' | 'started' | 'completed' | 'verified' | 'backlog' | 'archive';
export type StatusBadgeAppearance = 'filled' | 'plain';

export interface StatusBadgeProps {
  status: TicketStatus;
  showIcon?: boolean;
  appearance?: StatusBadgeAppearance;
  compact?: boolean;
  interactive?: boolean;
  actionLabel?: string;
  slot?: string;
}

const presentation: Record<TicketStatus, { icon: IconNode; iconName: string; label: string }> = {
  not_started: { icon: Circle, iconName: 'circle', label: 'Not started' },
  started: { icon: Clock, iconName: 'clock', label: 'Started' },
  completed: { icon: CircleCheck, iconName: 'circle-check', label: 'Completed' },
  verified: { icon: BadgeCheck, iconName: 'badge-check', label: 'Verified' },
  backlog: { icon: Clock3, iconName: 'clock-3', label: 'Backlog' },
  archive: { icon: Archive, iconName: 'archive', label: 'Archive' },
};

export function statusPresentation(status: TicketStatus) {
  return presentation[status];
}

export function StatusBadge({ status, showIcon = true, appearance = 'filled', compact = false, interactive = false, actionLabel, slot }: StatusBadgeProps) {
  const value = statusPresentation(status);
  const className = `status-badge status-badge--${status} status-badge--${appearance}${compact ? ' status-badge--compact' : ''}${interactive ? ' status-badge--interactive' : ''}`;
  const content = <>{showIcon && <LucideIcon class="status-badge__icon" icon={value.icon} name={value.iconName} />}<span>{value.label}</span></>;
  return interactive
    ? <button type="button" slot={slot} class={className} data-component="status-badge" data-status={status} data-appearance={appearance} aria-label={actionLabel}>{content}</button>
    : <span slot={slot} class={className} data-component="status-badge" data-status={status} data-appearance={appearance}>{content}</span>;
}

export function BlockedBadge({ compact = false }: { compact?: boolean }) {
  return <span class={`blocked-badge${compact ? ' blocked-badge--compact' : ''}`} data-component="blocked-badge">Blocked</span>;
}
