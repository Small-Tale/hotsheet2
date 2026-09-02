import { describe, expect, it } from 'vitest';

import { StatusBadge, statusPresentation, type TicketStatus } from './status-badge';

describe('StatusBadge', () => {
  it('maps every supported status to stable readable presentation', () => {
    const statuses: TicketStatus[] = ['not_started', 'started', 'completed', 'verified', 'backlog', 'archive'];
    expect(statuses.map(status => statusPresentation(status).label)).toEqual([
      'Not started', 'Started', 'Completed', 'Verified', 'Backlog', 'Archive',
    ]);
    expect(statuses.map(status => statusPresentation(status).iconName)).toEqual([
      'circle', 'clock', 'circle-check', 'badge-check', 'clock-3', 'archive',
    ]);
    expect(statuses.every(status => statusPresentation(status).icon.length > 0)).toBe(true);
  });

  it('offers independent plain and compact variants without changing readable status text', () => {
    const markup = String(StatusBadge({ status: 'started', appearance: 'plain', compact: true }));
    expect(markup).toContain('status-badge--plain');
    expect(markup).toContain('status-badge--compact');
    expect(markup).toContain('data-appearance="plain"');
    expect(markup).toContain('Started');
  });

  it('defaults to the filled non-compact presentation', () => {
    const markup = String(StatusBadge({ status: 'verified' }));
    expect(markup).toContain('status-badge--filled');
    expect(markup).not.toContain('status-badge--compact');
  });
});
