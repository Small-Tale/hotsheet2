import { describe, expect, it } from 'vitest';

import type { CheckoutAnalytics } from './api';
import { formatCost, recentThroughput, summarizeAnalytics } from './stats-summary';

const report: CheckoutAnalytics = {
  stores: [
    {
      store_id: 'shared',
      ticket_flow: {
        total: 7,
        open: 4,
        completed: 3,
        current_by_status: { started: 4, completed: 3 },
        current_by_category: { bug: 7 },
        throughput_by_day: { '2026-11-01': 2 },
        cycle_time_seconds: { samples: 2, average: 3600, p50: 3000, p95: 4200 },
        historical_cumulative_flow_available: false,
      },
      usage: {
        events: 3,
        tokens_in: 100,
        tokens_out: 50,
        cost_usd: 0.45,
        by_model: { model_a: 0.45 },
        by_ticket: {},
        by_day: {},
      },
    },
  ],
  unavailable_sources: [{ connection_id: 'jira', provider: 'jira' }],
};

describe('HS2-38RJMK checkout analytics summary', () => {
  it('counts a shared store once across projects and identifies missing provider data', () => {
    expect(summarizeAnalytics([report, report])).toMatchObject({
      stores: 1,
      total: 7,
      completed: 3,
      events: 3,
      costUsd: 0.45,
      averageCycleSeconds: 3600,
      unavailableSources: 1,
      historicalCumulativeFlowAvailable: false,
    });
  });

  it('fills missing UTC throughput dates without inventing completions', () => {
    const points = recentThroughput(report.stores[0].ticket_flow.throughput_by_day, new Date('2026-11-01T20:00:00Z'));
    expect(points).toHaveLength(14);
    expect(points.at(-1)).toEqual({ day: '2026-11-01', count: 2 });
    expect(points.at(-2)?.count).toBe(0);
  });

  it('weights average cycle time by completed samples across distinct stores', () => {
    const second: CheckoutAnalytics = {
      stores: [
        {
          ...report.stores[0],
          store_id: 'second',
          ticket_flow: {
            ...report.stores[0].ticket_flow,
            cycle_time_seconds: { samples: 1, average: 7200, p50: 7200, p95: 7200 },
          },
        },
      ],
      unavailable_sources: [],
    };
    expect(summarizeAnalytics([report, second])).toMatchObject({
      stores: 2,
      cycleSamples: 3,
      averageCycleSeconds: 4800,
      costUsd: 0.9,
    });
  });

  it('preserves small positive costs', () => {
    expect(formatCost(0.000000001)).toBe('< $0.00000001');
    expect(formatCost(0)).toBe('$0.00');
  });
});
