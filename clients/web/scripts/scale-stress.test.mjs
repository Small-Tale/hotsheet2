import { describe, expect, it } from 'vitest';

import { parseArguments, parseScaleCounts, syntheticTicket } from './scale-stress.mjs';

describe('scale stress harness', () => {
  it('normalizes incremental scale milestones', () => {
    expect(parseScaleCounts('100000, 10000,100000')).toEqual([10_000, 100_000]);
    expect(() => parseScaleCounts('10000,nope')).toThrow('Invalid --counts');
  });

  it('accepts repeatable-run switches', () => {
    expect(parseArguments(['--counts', '25,10', '--skip-web', '--keep', '--timeout-ms=4000', '--output', '/tmp/result.json'])).toMatchObject({
      counts: [10, 25], keep: true, skipWeb: true, timeoutMs: 4_000, output: '/tmp/result.json',
    });
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
