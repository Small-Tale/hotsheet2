import { describe, expect, it } from 'vitest';

import type { TicketUsage, TicketUsageEvent } from './api';
import { modelUsageCost, ticketUsageCost, timelineUsageLabels, usageModelsLabel } from './ticket-usage-timeline';

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
    // Unpriced events: the breakdown lists tokens per model by share, then the total.
    expect(labels.breakdown).toEqual([
      { label: 'opus', value: '23K tokens' },
      { label: 'sonnet', value: '4,000 tokens' },
      { label: 'Total', value: '27K tokens' },
    ]);
  });

  it("breaks the total down by model with each model's cost when it is known (HS2-J14225)", () => {
    const priced = usage([
      event('2026-10-01T10:30:00Z', 150_000, 'opus', 1.5),
      event('2026-10-01T11:30:00Z', 26_000, 'opus', 0.02),
      event('2026-10-01T11:40:00Z', 38_000, 'sonnet', 0.32),
    ]);
    expect(timelineUsageLabels(entries, priced).breakdown).toEqual([
      { label: 'opus', value: '176K tokens · ≈$1.52' },
      { label: 'sonnet', value: '38K tokens · ≈$0.32' },
      { label: 'Total', value: '214K tokens · ≈$1.84' },
    ]);
    // One unpriced sonnet event hides sonnet's cost and the total's, but not opus's.
    const partly = usage([
      event('2026-10-01T10:30:00Z', 1_000, 'opus', 0.5),
      event('2026-10-01T11:30:00Z', 2_000, 'sonnet'),
    ]);
    expect(modelUsageCost(partly, 'opus')).toBe(0.5);
    expect(modelUsageCost(partly, 'sonnet')).toBeUndefined();
    expect(modelUsageCost(partly, 'haiku')).toBeUndefined();
    expect(timelineUsageLabels(entries, partly).breakdown).toEqual([
      { label: 'sonnet', value: '2,000 tokens' },
      { label: 'opus', value: '1,000 tokens · ≈$0.50' },
      { label: 'Total', value: '3,000 tokens' },
    ]);
    // The settled rollup keeps no per-model split, so per-model cost is unknown once it holds any.
    const settled = usage([event('2026-10-02T10:00:00Z', 1_000, 'opus', 0.5)], {
      settled_cost_usd: 1.25,
      settled_through: '2026-10-01',
    });
    expect(modelUsageCost(settled, 'opus')).toBeUndefined();
    expect(timelineUsageLabels(entries, settled).breakdown).toEqual([
      { label: 'opus', value: '1,000 tokens' },
      { label: 'Total', value: '1,000 tokens · ≈$1.75' },
    ]);
  });

  it('is empty without usage and leaves gaps without events unlabeled', () => {
    expect(timelineUsageLabels(entries, undefined)).toEqual({
      perEntry: [undefined, undefined, undefined],
      breakdown: [],
    });
    expect(timelineUsageLabels(entries, usage([]))).toEqual({
      perEntry: [undefined, undefined, undefined],
      breakdown: [],
    });
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
      breakdown: [{ label: 'Total', value: '≈$3.00' }],
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
      breakdown: [
        { label: 'opus', value: '12K tokens' },
        { label: 'Total', value: '12K tokens' },
      ],
    });
  });
});
