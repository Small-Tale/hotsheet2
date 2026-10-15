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
}

interface UsageBucket {
  tokens: number;
  byModel: Map<string, number>;
}

function emptyBucket(): UsageBucket {
  return { tokens: 0, byModel: new Map() };
}

function add(bucket: UsageBucket, event: TicketUsageEvent) {
  const tokens = event.tokens_in + event.tokens_out;
  bucket.tokens += tokens;
  const model = event.model ?? event.tool;
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

/** Sum the event costs that are not already inside the settled rollup. */
export function ticketUsageCost(usage: TicketUsage): number | undefined {
  const live = usage.events.filter((event) => event.ts.slice(0, 10) > usage.settled_through);
  const priced = live.filter((event) => event.cost_usd !== undefined);
  if (!usage.settled_cost_usd && !priced.length) return undefined;
  if (priced.length !== live.length) return undefined;
  return usage.settled_cost_usd + priced.reduce((sum, event) => sum + event.cost_usd!, 0);
}

export function timelineUsageLabels(
  entries: readonly { timestamp: string }[],
  usage: TicketUsage | undefined,
): TimelineUsageLabels {
  const perEntry: (string | undefined)[] = entries.map(() => undefined);
  if (!usage || (!usage.events.length && !usage.settled_cost_usd)) return { perEntry };
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
