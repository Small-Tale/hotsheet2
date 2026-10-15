import { formatConversationCost, formatConversationTokens } from './ai-conversation';
import type { TicketUsage, TicketUsageEvent } from './api';

/**
 * Per-gap and total AI usage for the ticket timeline (HS2-ZA7W6E, HS2-J14225). Usage events
 * are attributed to the gap that was open when they were recorded: the gap after the newest
 * timeline entry at or before the event. Events older than the first entry count toward the
 * first gap, and events after the last entry count toward the last entry (the phase that is
 * still open).
 */
export interface TimelineUsageLabels {
  /** One label per timeline entry: the usage recorded between it and the next entry. */
  perEntry: (string | undefined)[];
  /** The whole ticket's usage, or `undefined` when nothing was recorded. */
  total?: string;
  /** The total broken down under the timeline: one row per model by descending token share, each
   * with its cost when that model's cost is known, then a Total row (HS2-J14225). Empty when
   * nothing was recorded. */
  breakdown: TimelineUsageRow[];
}

export interface TimelineUsageRow {
  label: string;
  value: string;
}

interface UsageBucket {
  tokens: number;
  byModel: Map<string, number>;
}

function emptyBucket(): UsageBucket {
  return { tokens: 0, byModel: new Map() };
}

function usageModel(event: TicketUsageEvent) {
  return event.model ?? event.tool;
}

function add(bucket: UsageBucket, event: TicketUsageEvent) {
  const tokens = event.tokens_in + event.tokens_out;
  bucket.tokens += tokens;
  const model = usageModel(event);
  bucket.byModel.set(model, (bucket.byModel.get(model) ?? 0) + tokens);
}

/** Models by descending token share; at most two named, the rest counted. */
export function usageModelsLabel(byModel: ReadonlyMap<string, number>): string {
  const ranked = [...byModel.entries()].sort((left, right) => right[1] - left[1] || left[0].localeCompare(right[0]));
  const named = ranked.slice(0, 2).map(([model]) => model);
  return ranked.length > 2 ? `${named.join(', ')} +${ranked.length - 2}` : named.join(', ');
}

function bucketLabel(bucket: UsageBucket): string | undefined {
  if (!bucket.tokens && !bucket.byModel.size) return undefined;
  const models = usageModelsLabel(bucket.byModel);
  return `${formatConversationTokens(bucket.tokens)} tokens${models ? ` · ${models}` : ''}`;
}

function liveEvents(usage: TicketUsage) {
  return usage.events.filter((event) => event.ts.slice(0, 10) > usage.settled_through);
}

/** Sum the event costs that are not already inside the settled rollup. */
export function ticketUsageCost(usage: TicketUsage): number | undefined {
  const live = liveEvents(usage);
  const priced = live.filter((event) => event.cost_usd !== undefined);
  if (!usage.settled_cost_usd && !priced.length) return undefined;
  if (priced.length !== live.length) return undefined;
  return usage.settled_cost_usd + priced.reduce((sum, event) => sum + event.cost_usd!, 0);
}

/**
 * One model's cost, known only while no settled rollup hides a share of it (the rollup keeps no
 * per-model split) and every one of its live events is priced.
 */
export function modelUsageCost(usage: TicketUsage, model: string): number | undefined {
  if (usage.settled_cost_usd) return undefined;
  const events = liveEvents(usage).filter((event) => usageModel(event) === model);
  if (!events.length || events.some((event) => event.cost_usd === undefined)) return undefined;
  return events.reduce((sum, event) => sum + event.cost_usd!, 0);
}

function usageBreakdown(usage: TicketUsage, total: UsageBucket, cost: number | undefined): TimelineUsageRow[] {
  const withCost = (tokens: string, value: number | undefined) =>
    value === undefined
      ? tokens
      : tokens
        ? `${tokens} · ${formatConversationCost(value)}`
        : formatConversationCost(value);
  const rows = [...total.byModel.entries()]
    .sort((left, right) => right[1] - left[1] || left[0].localeCompare(right[0]))
    .map(([model, tokens]) => ({
      label: model,
      value: withCost(`${formatConversationTokens(tokens)} tokens`, modelUsageCost(usage, model)),
    }));
  const totalValue = withCost(total.tokens ? `${formatConversationTokens(total.tokens)} tokens` : '', cost);
  return totalValue ? [...rows, { label: 'Total', value: totalValue }] : rows;
}

export function timelineUsageLabels(
  entries: readonly { timestamp: string }[],
  usage: TicketUsage | undefined,
): TimelineUsageLabels {
  const perEntry: (string | undefined)[] = entries.map(() => undefined);
  if (!usage || (!usage.events.length && !usage.settled_cost_usd)) return { perEntry, breakdown: [] };
  const buckets = entries.map(emptyBucket),
    total = emptyBucket();
  const events = [...usage.events].sort((left, right) => left.ts.localeCompare(right.ts));
  let index = 0;
  for (const event of events) {
    while (index + 1 < entries.length && entries[index + 1].timestamp <= event.ts) index += 1;
    if (buckets.length) add(buckets[index], event);
    add(total, event);
  }
  const cost = ticketUsageCost(usage),
    totalLabel = bucketLabel(total);
  return {
    perEntry: buckets.map(bucketLabel),
    breakdown: usageBreakdown(usage, total, cost),
    total:
      totalLabel === undefined
        ? cost === undefined
          ? undefined
          : formatConversationCost(cost)
        : cost === undefined
          ? totalLabel
          : `${totalLabel.replace(/ · .*$/, '')} · ${formatConversationCost(cost)}${
              usageModelsLabel(total.byModel) ? ` · ${usageModelsLabel(total.byModel)}` : ''
            }`,
  };
}
