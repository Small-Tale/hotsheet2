import { describe, expect, it } from 'vitest';

import { priorityPresentation, TICKET_PRIORITIES } from './priority-presentation';
import { TicketPrioritySelect } from './ticket-priority-select';
import { getPriorityPresentation } from './ticket-row';

describe('shared priority presentation (HS2-8MT5KG)', () => {
  it('orders priorities from most to least urgent with Lucide icons and app color tokens', () => {
    expect(TICKET_PRIORITIES.map((priority) => priority.value)).toEqual(['urgent', 'high', 'default', 'low']);
    expect(priorityPresentation('urgent')).toMatchObject({ iconName: 'chevrons-up', colorToken: '--hs-priority-urgent' });
    expect(priorityPresentation('low')).toMatchObject({ label: 'Low', iconName: 'chevron-down' });
    expect(priorityPresentation('bogus' as never).value).toBe('default');
  });

  it('feeds both the TicketRow indicator and the TicketPrioritySelect choices', () => {
    for (const { value, icon, iconName, colorToken } of TICKET_PRIORITIES) {
      expect(getPriorityPresentation(value)).toEqual({ icon, name: iconName, color: `var(${colorToken})` });
    }
    const select = String(TicketPrioritySelect({ name: 'priority', value: 'high' }));
    for (const { label, iconName } of TICKET_PRIORITIES) {
      expect(select).toContain(label);
      expect(select).toContain(`data-lucide="${iconName}"`);
    }
  });
});
