import { describe, expect, it } from 'vitest';

import { ticketCompletionTrend } from './ticket-completion-trend';

function localTimestamp(day: number, hour = 12): string {
  return new Date(2026, 8, day, hour).toISOString();
}

describe('ticket completion trend', () => {
  it('counts the last seven local calendar days from oldest through today', () => {
    const trend = ticketCompletionTrend([
      { completed_at: localTimestamp(1) },
      { completed_at: localTimestamp(3) },
      { completed_at: localTimestamp(3, 18) },
      { completed_at: localTimestamp(7) },
      { completed_at: localTimestamp(8) },
      { completed_at: 'not-a-date' },
      {},
    ], new Date(2026, 8, 7, 20));

    expect(trend).toEqual([1, 0, 2, 0, 0, 0, 1]);
  });

  it('supports explicit window sizes without manufacturing days', () => {
    expect(ticketCompletionTrend([{ completed_at: localTimestamp(7) }], new Date(2026, 8, 7), 1)).toEqual([1]);
    expect(ticketCompletionTrend([], new Date(2026, 8, 7), 0)).toEqual([]);
  });
});
