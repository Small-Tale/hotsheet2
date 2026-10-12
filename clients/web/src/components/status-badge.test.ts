import { describe, expect, it } from 'vitest';

import { BlockedBadge, StatusBadge, statusPresentation, type TicketStatus } from './status-badge';

describe('StatusBadge', () => {
  it('tints each status through a Kerf Badge tone and renders Blocked as a solid Badge (HS2-3FMK0T)', () => {
    for (const [status, tone] of [
      ['not_started', 'neutral'],
      ['started', 'warning'],
      ['completed', 'success'],
      ['verified', 'info'],
      ['backlog', 'neutral'],
    ] as const)
      expect(String(StatusBadge({ status }))).toContain(`data-tone="${tone}"`);
    expect(String(StatusBadge({ status: 'started', compact: true }))).toContain('data-size="compact"');
    expect(String(BlockedBadge({ compact: true }))).toMatch(
      /data-appearance="solid"[^>]*data-size="compact"[^>]*>Blocked/u,
    );
    const trigger = String(StatusBadge({ status: 'started', interactive: true, actionLabel: 'Change status' }));
    expect(trigger).toMatch(/^<button type="button" class="status-badge"/u);
    expect(trigger).toContain('data-interactive="true"');
  });

  it('maps every supported status to stable readable presentation', () => {
    const statuses: TicketStatus[] = [
      'not_started',
      'started',
      'completed',
      'verified',
      'backlog',
      'archive',
      'deleted',
    ];
    expect(statuses.map((status) => statusPresentation(status).label)).toEqual([
      'Not started',
      'Started',
      'Completed',
      'Verified',
      'Backlog',
      'Archive',
      'Deleted',
    ]);
    expect(statuses.map((status) => statusPresentation(status).iconName)).toEqual([
      'circle',
      'clock',
      'circle-check',
      'badge-check',
      'clock-3',
      'archive',
      'trash-2',
    ]);
    expect(statuses.every((status) => statusPresentation(status).icon.length > 0)).toBe(true);
  });

  it('offers independent plain and compact variants without changing readable status text', () => {
    const markup = String(StatusBadge({ status: 'started', appearance: 'plain', compact: true }));
    expect(markup).toContain('data-appearance="plain"');
    expect(markup).toContain('data-compact="true"');
    // Plain is the bare icon and quiet label: no Kerf Badge pill (HS2-3FMK0T).
    expect(markup).not.toContain('data-component="badge"');
    expect(markup).toContain('Started');
  });

  it('defaults to the filled non-compact presentation', () => {
    const markup = String(StatusBadge({ status: 'verified' }));
    expect(markup).toContain('data-appearance="filled"');
    expect(markup).not.toContain('data-compact');
    expect(markup).toMatch(/data-component="badge" data-tone="info"[^>]*data-size="default"/u);
  });

  it('shows a Started phase without changing the lifecycle status', () => {
    const waiting = String(StatusBadge({ status: 'started', startedPhase: 'final_testing', compact: true }));
    expect(waiting).toContain('Final testing');
    expect(waiting).toContain('data-status="started"');
    expect(String(StatusBadge({ status: 'completed', startedPhase: 'final_testing' }))).toContain('Completed');
  });
});
