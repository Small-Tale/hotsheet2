import { describe, expect, it } from 'vitest';

import type { TicketUsage, TicketUsageEvent } from './api';
import { ticketUsageCost, timelineUsageLabels, usageModelsLabel } from './ticket-usage-timeline';

const event = (ts: string, tokens: number, model?: string, cost?: number): TicketUsageEvent => ({
  ts,
  tool: 'claude',
  ...(model ? { model } : {}),
  tokens_in: tokens - 10,
  tokens_out: 10,
  ...(cost === undefined ? {} : { cost_usd: cost }),
});
const usage = (events: TicketUsageEvent[], settled: Partial<TicketUsage> = {}): TicketUsage => ({
  events,
  settled_cost_usd: 0,
  settled_through: '',
  ...settled,
});
const entries = [
  { timestamp: '2026-10-01T10:00:00Z' },
  { timestamp: '2026-10-01T11:00:00Z' },
  { timestamp: '2026-10-01T12:00:00Z' },
];

describe('timelineUsageLabels (HS2-ZA7W6E, HS2-J14225)', () => {
  it('attributes each event to the gap open when it was recorded, including the boundaries', () => {
    const labels = timelineUsageLabels(
      entries,
      usage([
        event('2026-10-01T09:00:00Z', 1_000, 'opus'), // before the first entry: first gap
        event('2026-10-01T10:30:00Z', 2_000, 'opus'),
        event('2026-10-01T11:00:00Z', 4_000, 'sonnet'), // exactly on a boundary: the gap it opens
        event('2026-10-01T12:30:00Z', 20_000, 'opus'), // after the last entry: the open phase
      ]),
    );
    expect(labels.perEntry).toEqual(['3,000 tokens · opus', '4,000 tokens · sonnet', '20K tokens · opus']);
    expect(labels.total).toBe('27K tokens · opus, sonnet');
  });

  it('is empty without usage and leaves gaps without events unlabeled', () => {
    expect(timelineUsageLabels(entries, undefined)).toEqual({ perEntry: [undefined, undefined, undefined] });
    expect(timelineUsageLabels(entries, usage([]))).toEqual({ perEntry: [undefined, undefined, undefined] });
    const labels = timelineUsageLabels(entries, usage([event('2026-10-01T11:30:00Z', 500, 'opus')]));
    expect(labels.perEntry).toEqual([undefined, '500 tokens · opus', undefined]);
  });

  it('adds cost to the total only when every live event is priced, and keeps settled cost after pruning', () => {
    const priced = usage([event('2026-10-02T10:00:00Z', 1_000, 'opus', 0.5)], {
      settled_cost_usd: 1.25,
      settled_through: '2026-10-01',
    });
    expect(ticketUsageCost(priced)).toBeCloseTo(1.75);
    expect(timelineUsageLabels([{ timestamp: '2026-10-01T00:00:00Z' }], priced).total).toBe(
      '1,000 tokens · ≈$1.75 · opus',
    );
    // An unpriced live event makes the cost unknowable; tokens still show.
    const mixed = usage([event('2026-10-02T10:00:00Z', 1_000, 'opus', 0.5), event('2026-10-02T11:00:00Z', 1_000)]);
    expect(ticketUsageCost(mixed)).toBeUndefined();
    expect(timelineUsageLabels([{ timestamp: '2026-10-01T00:00:00Z' }], mixed).total).toBe(
      '2,000 tokens · claude, opus',
    );
    // Only settled cost survives once every raw event is pruned.
    const settledOnly = usage([], { settled_cost_usd: 3, settled_through: '2026-10-01' });
    expect(timelineUsageLabels(entries, settledOnly)).toEqual({
      perEntry: [undefined, undefined, undefined],
      total: '≈$3.00',
    });
    // Events inside the settled window are already counted in settled cost.
    const overlapping = usage([event('2026-10-01T10:00:00Z', 1_000, 'opus', 9)], {
      settled_cost_usd: 1,
      settled_through: '2026-10-01',
    });
    expect(ticketUsageCost(overlapping)).toBe(1);
  });

  it('names at most two models by token share and counts the rest', () => {
    expect(
      usageModelsLabel(
        new Map([
          ['sonnet', 5],
          ['opus', 50],
          ['haiku', 1],
        ]),
      ),
    ).toBe('opus, sonnet +1');
    expect(
      usageModelsLabel(
        new Map([
          ['b', 1],
          ['a', 1],
        ]),
      ),
    ).toBe('a, b');
    expect(usageModelsLabel(new Map())).toBe('');
  });

  it('handles tickets without timeline entries by reporting only the total', () => {
    expect(timelineUsageLabels([], usage([event('2026-10-01T10:00:00Z', 12_000, 'opus')]))).toEqual({
      perEntry: [],
      total: '12K tokens · opus',
    });
  });
});
