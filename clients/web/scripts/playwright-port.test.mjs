import { createServer } from 'node:net';

import { describe, expect, it } from 'vitest';

import { freeLoopbackPort, playwrightServerPort } from './playwright-port.mjs';

describe('Playwright fixture port (HS2-6WH4PQ)', () => {
  it('honors an explicit port and never overwrites it', () => {
    const env = { HOTSHEET_WEB_TEST_PORT: '4186' };
    expect(playwrightServerPort(env, () => 1)).toBe(4186);
    expect(env.HOTSHEET_WEB_TEST_PORT).toBe('4186');
  });

  it('rejects an explicit value that is not a TCP port', () => {
    for (const value of ['abc', '0', '70000', '41.5'])
      expect(() => playwrightServerPort({ HOTSHEET_WEB_TEST_PORT: value }, () => 1)).toThrow(/must be a TCP port/);
  });

  it('picks once in the runner and hands the same port to the workers that inherit its environment', () => {
    const runner = {};
    let picks = 0;
    const port = playwrightServerPort(runner, () => {
      picks += 1;
      return 50_123;
    });
    expect(port).toBe(50_123);
    expect(runner.HOTSHEET_WEB_TEST_PORT).toBe('50123');
    // A worker reloads the config with the runner's environment and must not pick again.
    expect(playwrightServerPort({ ...runner }, () => 1)).toBe(50_123);
    expect(picks).toBe(1);
  });

  it('allocates a port that is actually free on loopback, distinct for concurrent runs', async () => {
    const first = playwrightServerPort({}),
      second = playwrightServerPort({});
    expect(first).not.toBe(second);
    for (const port of [first, second, freeLoopbackPort()]) {
      const server = createServer();
      await new Promise((resolve, reject) => server.once('error', reject).listen(port, '127.0.0.1', resolve));
      await new Promise((resolve) => server.close(resolve));
    }
  });
});
