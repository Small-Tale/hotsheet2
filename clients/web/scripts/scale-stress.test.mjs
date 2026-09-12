import { describe, expect, it } from 'vitest';

import { assertCliMutationBudgets, assertCliReadBudgets, assertReindexBudgets, assertWeb100kAcceptance, parseArguments, parseScaleCounts, syntheticTicket } from './scale-stress.mjs';

describe('scale stress harness', () => {
  it('normalizes incremental scale milestones', () => {
    expect(parseScaleCounts('100000, 10000,100000')).toEqual([10_000, 100_000]);
    expect(() => parseScaleCounts('10000,nope')).toThrow('Invalid --counts');
  });

  it('accepts repeatable-run switches', () => {
    expect(parseArguments(['--counts', '25,10', '--skip-web', '--assert-cli-budgets', '--assert-cli-mutation-budgets', '--assert-reindex-budgets', '--assert-web-100k', '--keep', '--timeout-ms=4000', '--output', '/tmp/result.json'])).toMatchObject({
      counts: [10, 25], keep: true, skipWeb: true, assertCliBudgets: true, assertCliMutationBudgets: true, assertReindexBudgets: true, assertWeb100k: true, timeoutMs: 4_000, output: '/tmp/result.json',
    });
  });

  it('enforces opt-in full-reindex budgets at the 100K and 1M tiers', () => {
    expect(() => assertReindexBudgets(100_000, { reindex: { wall_ms: 59_999 } })).not.toThrow();
    expect(() => assertReindexBudgets(100_000, { reindex: { wall_ms: 60_001 } })).toThrow('reindex');
    expect(() => assertReindexBudgets(1_000_000, { reindex: { timed_out: true } })).toThrow('reindex');
    expect(() => assertReindexBudgets(10_000, {})).not.toThrow();
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

  it('enforces the opt-in 100K browser and bounded-page acceptance thresholds', () => {
    const page = {wall_ms:59_999,response_bytes:999_999,item_count:200,has_next_cursor:true};
    const server = {scenarios:{list_compact:page,list_compact_next:page}};
    const web = {scenarios:{initial_load:{wall_ms:119_999},switch_backlog:{wall_ms:1_999},switch_archive:{wall_ms:1_999},switch_queue:{wall_ms:1_999},browser_heap_mb:191.9}};
    expect(() => assertWeb100kAcceptance(100_000,server,web)).not.toThrow();
    expect(() => assertWeb100kAcceptance(100_000,{scenarios:{...server.scenarios,list_compact:{...page,response_bytes:1_000_001}}},web)).toThrow('bounded-page');
    expect(() => assertWeb100kAcceptance(100_000,server,{scenarios:{...web.scenarios,browser_heap_mb:193}})).toThrow('initial load/heap');
    expect(() => assertWeb100kAcceptance(10_000,{},{})).not.toThrow();
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
