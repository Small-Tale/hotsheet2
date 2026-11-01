import type { CheckoutAnalytics } from './api';

export interface StatsSummary {
  stores: number;
  total: number;
  open: number;
  completed: number;
  byStatus: Record<string, number>;
  byCategory: Record<string, number>;
  throughputByDay: Record<string, number>;
  cycleSamples: number;
  averageCycleSeconds: number;
  events: number;
  tokensIn: number;
  tokensOut: number;
  costUsd: number;
  costByModel: Record<string, number>;
  unavailableSources: number;
  historicalCumulativeFlowAvailable: boolean;
}

function addMap(target: Record<string, number>, source: Record<string, number>) {
  for (const [key, value] of Object.entries(source)) target[key] = (target[key] ?? 0) + value;
}

/** Combine checkout reports without counting a Git store linked to two projects twice. */
export function summarizeAnalytics(reports: readonly CheckoutAnalytics[]): StatsSummary {
  const summary: StatsSummary = {
    stores: 0,
    total: 0,
    open: 0,
    completed: 0,
    byStatus: {},
    byCategory: {},
    throughputByDay: {},
    cycleSamples: 0,
    averageCycleSeconds: 0,
    events: 0,
    tokensIn: 0,
    tokensOut: 0,
    costUsd: 0,
    costByModel: {},
    unavailableSources: 0,
    historicalCumulativeFlowAvailable: false,
  };
  const seen = new Set<string>();
  const unavailable = new Set<string>();
  let cycleSeconds = 0;
  let allHaveHistory = true;
  for (const report of reports) {
    for (const source of report.unavailable_sources) unavailable.add(`${source.provider}:${source.connection_id}`);
    for (const { store_id: storeId, ticket_flow: flow, usage } of report.stores) {
      if (seen.has(storeId)) continue;
      seen.add(storeId);
      summary.stores += 1;
      summary.total += flow.total;
      summary.open += flow.open;
      summary.completed += flow.completed;
      addMap(summary.byStatus, flow.current_by_status);
      addMap(summary.byCategory, flow.current_by_category);
      addMap(summary.throughputByDay, flow.throughput_by_day);
      summary.cycleSamples += flow.cycle_time_seconds.samples;
      cycleSeconds += flow.cycle_time_seconds.average * flow.cycle_time_seconds.samples;
      summary.events += usage.events;
      summary.tokensIn += usage.tokens_in;
      summary.tokensOut += usage.tokens_out;
      summary.costUsd += usage.cost_usd;
      addMap(summary.costByModel, usage.by_model);
      allHaveHistory &&= flow.historical_cumulative_flow_available;
    }
  }
  summary.averageCycleSeconds = summary.cycleSamples ? cycleSeconds / summary.cycleSamples : 0;
  summary.unavailableSources = unavailable.size;
  summary.historicalCumulativeFlowAvailable = summary.stores > 0 && allHaveHistory;
  return summary;
}

/** Fourteen complete UTC calendar days, ending at `now`; gaps are genuine zero days. */
export function recentThroughput(
  byDay: Readonly<Record<string, number>>,
  now: Date,
): Array<{ day: string; count: number }> {
  const end = Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), now.getUTCDate());
  return Array.from({ length: 14 }, (_, offset) => {
    const day = new Date(end - (13 - offset) * 86_400_000).toISOString().slice(0, 10);
    return { day, count: byDay[day] ?? 0 };
  });
}

export function formatCycleTime(seconds: number, samples: number): string {
  if (!samples) return 'No completed cycle samples';
  if (seconds < 60) return '< 1 min';
  if (seconds < 3_600) return `${Math.round(seconds / 60)} min`;
  if (seconds < 86_400) return `${(seconds / 3_600).toFixed(1).replace(/\.0$/u, '')} hr`;
  return `${(seconds / 86_400).toFixed(1).replace(/\.0$/u, '')} days`;
}

export function formatCost(usd: number): string {
  if (usd > 0 && usd < 0.00000001) return '< $0.00000001';
  return `$${usd.toLocaleString('en-US', { minimumFractionDigits: 2, maximumFractionDigits: 8 })}`;
}
