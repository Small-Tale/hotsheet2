import { describe, expect, it } from 'vitest';

import type { CheckoutAnalytics } from '../api';
import { StatsDashboard } from './stats-dashboard';

const emptyReport: CheckoutAnalytics = { stores: [], unavailable_sources: [] };

describe('HS2-38RJMK StatsDashboard states', () => {
  it('shows an empty store state without presenting zero as a complete portfolio total', () => {
    const html = String(
      StatsDashboard({
        projects: [{ id: 'one', name: 'One', state: { status: 'ready', report: emptyReport } }],
      }),
    );
    expect(html).toContain('No Linked Git Ticket Stores');
    expect(html).not.toContain('stats-dashboard__metrics');
  });

  it('names failed and unsupported provider sources beside partial data', () => {
    const html = String(
      StatsDashboard({
        projects: [
          {
            id: 'one',
            name: 'One',
            state: {
              status: 'ready',
              report: {
                ...emptyReport,
                unavailable_sources: [{ connection_id: 'provider', provider: 'jira' }],
              },
            },
          },
          { id: 'two', name: 'Two', state: { status: 'error', message: 'Unavailable in this checkout' } },
        ],
        crossProject: true,
      }),
    );
    expect(html).toContain('One');
    expect(html).toContain('Two Analytics Unavailable');
    expect(html).toContain('Some Ticket Sources Are Unavailable');
    expect(html).toContain('excluded from these totals');
  });
});
