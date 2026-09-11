import { describe, expect, it } from 'vitest';

import { assertCliMutationBudgets, assertCliReadBudgets, parseArguments, parseScaleCounts, syntheticTicket } from './scale-stress.mjs';

describe('scale stress harness', () => {
  it('normalizes incremental scale milestones', () => {
    expect(parseScaleCounts('100000, 10000,100000')).toEqual([10_000, 100_000]);
    expect(() => parseScaleCounts('10000,nope')).toThrow('Invalid --counts');
  });

  it('accepts repeatable-run switches', () => {
    expect(parseArguments(['--counts', '25,10', '--skip-web', '--assert-cli-budgets', '--assert-cli-mutation-budgets', '--keep', '--timeout-ms=4000', '--output', '/tmp/result.json'])).toMatchObject({
      counts: [10, 25], keep: true, skipWeb: true, assertCliBudgets: true, assertCliMutationBudgets: true, timeoutMs: 4_000, output: '/tmp/result.json',
    });
  });

  it('enforces opt-in bounded CLI mutation budgets at the 10K and 100K tiers', () => {
    const within = { create_ticket: { wall_ms: 4_999 }, modify_ticket: { wall_ms: 4_999 } };
    expect(() => assertCliMutationBudgets(10_000, within)).not.toThrow();
    expect(() => assertCliMutationBudgets(10_000, { ...within, create_ticket: { wall_ms: 5_001 } })).toThrow('create_ticket');
    expect(() => assertCliMutationBudgets(100_000, { ...within, modify_ticket: { timed_out: true } })).toThrow('modify_ticket');
  });

  it('enforces opt-in bounded CLI read budgets at the 10K and 100K tiers', () => {
    const within = Object.fromEntries(['list_first_100', 'full_text_query', 'show_ticket'].map(name => [name, { wall_ms: 1_999 }]));
    expect(() => assertCliReadBudgets(10_000, within)).not.toThrow();
    expect(() => assertCliReadBudgets(10_000, { ...within, show_ticket: { wall_ms: 2_001 } })).toThrow('show_ticket');
    expect(() => assertCliReadBudgets(100_000, { ...within, full_text_query: { error: 'timeout' } })).toThrow('full_text_query');
    expect(() => assertCliReadBudgets(1_000, {})).not.toThrow();
  });

  it('generates unique canonical ticket fixtures across every shipped view', () => {
    const tickets = [1, 2, 3, 4].map(index => syntheticTicket(index));
    expect(new Set(tickets.map(ticket => ticket.id)).size).toBe(4);
    expect(new Set(tickets.map(ticket => ticket.slug)).size).toBe(4);
    expect(tickets.map(ticket => ticket.body.match(/status: ([a-z_]+)/)?.[1])).toEqual(['archive', 'started', 'not_started', 'backlog']);
    expect(tickets[2].body).toContain('up_next: true');
    expect(tickets.every(ticket => ticket.body.includes('schema: hotsheet/v2-bounded-notes'))).toBe(true);
  });
});
