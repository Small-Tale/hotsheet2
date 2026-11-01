import { CatalogExample } from '@kerfjs/ui/catalog';

import type { CheckoutAnalytics } from '../api';
import { StatsDashboard } from '../components/stats-dashboard';
import type { DemoModule } from './demo-module';

const report: CheckoutAnalytics = {
  stores: [
    {
      store_id: 'demo-store',
      ticket_flow: {
        total: 12,
        open: 7,
        completed: 5,
        current_by_status: { 'Not Started': 3, Started: 4, Completed: 5 },
        current_by_category: { feature: 6, bug: 4, task: 2 },
        throughput_by_day: { '2026-11-01': 2, '2026-10-30': 1, '2026-10-27': 2 },
        cycle_time_seconds: { samples: 5, average: 172800, p50: 86400, p95: 345600 },
        historical_cumulative_flow_available: false,
      },
      usage: {
        events: 8,
        tokens_in: 120000,
        tokens_out: 35000,
        cost_usd: 2.48,
        by_model: { 'model-a': 1.74, 'model-b': 0.74 },
        by_ticket: {},
        by_day: {},
      },
    },
  ],
  unavailable_sources: [{ connection_id: 'external', provider: 'github' }],
};

export const demoModule: DemoModule = {
  entries: {
    'stats-dashboard': {
      catalogExamples: true,
      Demo: () => (
        <CatalogExample label="Cross-Project Analytics" viewport={{ width: 'full' }}>
          <StatsDashboard
            projects={[{ id: 'demo', name: 'Demo', state: { status: 'ready', report } }]}
            crossProject
            now={new Date('2026-11-01T12:00:00Z')}
          />
        </CatalogExample>
      ),
    },
  },
};
