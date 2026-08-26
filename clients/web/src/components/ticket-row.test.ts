import { describe, expect, it } from 'vitest';
import { getPriorityPresentation, normalizeTicketRowProps, TicketRow, ticketRowIndicator } from './ticket-row';

describe('TicketRow', () => {
  it('normalizes fallbacks, tags, and boolean defaults', () => {
    expect(normalizeTicketRowProps({
      slug: ' ', title: ' ', status: 'not_started', priority: 'default', category: ' ', tags: [' client ', '', ' ux '],
    })).toEqual({
      slug: 'HS2-UNKNOWN', title: 'Untitled ticket', status: 'not_started', priority: 'default',
      category: 'issue', tags: ['client', 'ux'], upNext: false, selected: false, busy: false,
      blocked: false, needsReview: false, agentName: 'AI', updatedLabel: 'Recently',
    });
  });

  it('reserves the indicator rail for special states in HS1 precedence order', () => {
    expect(ticketRowIndicator({})).toBeUndefined();
    expect(ticketRowIndicator({ upNext: true })).toBe('up-next');
    expect(ticketRowIndicator({ upNext: true, blocked: true })).toBe('blocked');
    expect(ticketRowIndicator({ upNext: true, blocked: true, needsReview: true })).toBe('needs-review');
  });

  it('maps HS2 priorities onto the HS1 icon and color semantics', () => {
    expect(Object.fromEntries((['urgent', 'high', 'default', 'low'] as const).map(priority => {
      const presentation = getPriorityPresentation(priority);
      return [priority, [presentation.name, presentation.color]];
    }))).toEqual({
      urgent: ['chevrons-up', '#ef4444'], high: ['chevron-up', '#f97316'],
      default: ['minus', '#6b7280'], low: ['chevron-down', '#3b82f6'],
    });
  });

  it('establishes an outer width container so its descendant can enter narrow presentation', () => {
    const markup = String(TicketRow({ slug: 'HS2-WIDTH', title: 'Responsive row', status: 'started', priority: 'default', category: 'task', tags: [] }));
    expect(markup).toContain('data-component="ticket-list-row-container"');
    expect(markup.indexOf('ticket-list-row-container')).toBeLessThan(markup.indexOf('data-component="ticket-list-row"'));
  });
});
