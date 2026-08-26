import { describe, expect, it } from 'vitest';
import { statusPresentation, type TicketStatus } from './status-badge';

describe('StatusBadge', () => {
  it('maps every supported status to stable readable presentation', () => {
    const statuses: TicketStatus[] = ['not_started', 'started', 'completed', 'verified', 'backlog'];
    expect(statuses.map(status => statusPresentation(status).label)).toEqual([
      'Not started', 'Started', 'Completed', 'Verified', 'Backlog',
    ]);
    expect(statuses.every(status => statusPresentation(status).icon.length > 0)).toBe(true);
  });
});
