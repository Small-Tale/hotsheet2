export type TicketStatus = 'not_started' | 'started' | 'completed' | 'verified' | 'backlog';

export interface StatusBadgeProps {
  status: TicketStatus;
  showIcon?: boolean;
}

const presentation: Record<TicketStatus, { icon: string; label: string }> = {
  not_started: { icon: '○', label: 'Not started' },
  started: { icon: '◐', label: 'Started' },
  completed: { icon: '●', label: 'Completed' },
  verified: { icon: '✓', label: 'Verified' },
  backlog: { icon: '◇', label: 'Backlog' },
};

export function statusPresentation(status: TicketStatus) {
  return presentation[status];
}

export function StatusBadge({ status, showIcon = true }: StatusBadgeProps) {
  const value = statusPresentation(status);
  return (
    <span class={`status-badge status-badge--${status}`} data-component="status-badge" data-status={status}>
      {showIcon && <span class="status-badge__icon" aria-hidden="true">{value.icon}</span>}
      <span>{value.label}</span>
    </span>
  );
}
