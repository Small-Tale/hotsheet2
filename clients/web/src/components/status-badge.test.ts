import { describe, expect, it } from 'vitest';
import { StatusBadge, statusPresentation, type TicketStatus } from './status-badge';

describe('StatusBadge', () => {
  it('maps every supported status to stable readable presentation', () => {
    const statuses: TicketStatus[] = ['not_started', 'started', 'completed', 'verified', 'backlog'];
    expect(statuses.map(status => statusPresentation(status).label)).toEqual([
      'Not started', 'Started', 'Completed', 'Verified', 'Backlog',
    ]);
    expect(statuses.map(status => statusPresentation(status).iconName)).toEqual([
      'circle', 'clock', 'circle-check', 'badge-check', 'archive',
    ]);
    expect(statuses.every(status => statusPresentation(status).icon.length > 0)).toBe(true);
  });

  it('offers a compact row presentation without changing readable status text', () => {
    const markup = String(StatusBadge({ status: 'started', compact: true }));
    expect(markup).toContain('status-badge--compact');
    expect(markup).toContain('Started');
  });
});
