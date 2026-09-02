import { describe, expect, it, vi } from 'vitest';

import { loadProjectTicketRefresh } from './project-ticket-refresh';

describe('loadProjectTicketRefresh', () => {
  it('keeps healthy tickets when the corrupt-ticket index fails', async () => {
    const ticket = { id: '01', slug: 'HS2-OK', title: 'Healthy', tags: [] };
    const result = await loadProjectTicketRefresh({
      checkoutTickets: vi.fn().mockResolvedValue([ticket]),
      checkoutCorruptTickets: vi.fn().mockRejectedValue(new Error('index unavailable')),
    }, 'checkout');

    expect(result).toEqual({ tickets: [ticket], corruptTicketsError: 'index unavailable' });
  });

  it('keeps corrupt entries available when the healthy-ticket index fails', async () => {
    const corrupt = { store: 'local', store_path: '/tickets', path: '/tickets/01.md', slug: 'HS2-BAD', error: 'invalid notes' };
    const result = await loadProjectTicketRefresh({
      checkoutTickets: vi.fn().mockRejectedValue(new Error('healthy index unavailable')),
      checkoutCorruptTickets: vi.fn().mockResolvedValue([corrupt]),
    }, 'checkout');

    expect(result).toEqual({ ticketsError: 'healthy index unavailable', corruptTickets: [corrupt] });
  });

  it('removes a stale healthy row when live diagnostics identify the same slug as corrupt', async () => {
    const healthy = { id: '01', slug: 'HS2-OK', title: 'Healthy', tags: [] };
    const stale = { id: '02', slug: 'HS2-BAD', title: 'Stale indexed title', tags: [] };
    const corrupt = { store: 'local', store_path: '/tickets', path: '/tickets/02.md', slug: 'HS2-BAD', error: 'invalid notes' };
    const result = await loadProjectTicketRefresh({
      checkoutTickets: vi.fn().mockResolvedValue([healthy,stale]),
      checkoutCorruptTickets: vi.fn().mockResolvedValue([corrupt]),
    }, 'checkout');

    expect(result).toEqual({ tickets: [healthy], corruptTickets: [corrupt] });
  });
});
