import { describe, expect, it } from 'vitest';

import { traceDevBridgeRequest } from './dev-request-timing';

describe('development bridge request timing', () => {
  it('preserves the upstream response and reports queue and fetch stages without request data', async () => {
    const ticks = [10, 12, 19, 21],
      response = await traceDevBridgeRequest(
        async (mark) => {
          mark('dispatch');
          mark('headers');
          return new Response('ok', { headers: { 'server-timing': 'handler;dur=1' } });
        },
        () => ticks.shift() ?? 0,
        () => 'random-trace',
      );
    expect(await response.text()).toBe('ok');
    expect(response.headers.get('server-timing')).toBe(
      'handler;dur=1, bridge;dur=11.00, bridge_queue;dur=2.00, upstream;dur=7.00',
    );
    expect(response.headers.get('x-hotsheet-trace-id')).toBe('random-trace');
  });
});
