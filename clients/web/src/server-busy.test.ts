import { readFileSync } from 'node:fs';

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { ServerBusyBars } from './components/server-busy-bars';
import { beginServerRequest, computeServerBusyBarCount, endServerRequest, serverBusy, serverInFlightCount } from './server-busy';

describe('computeServerBusyBarCount', () => {
  it('returns zero for a non-positive or non-finite width', () => {
    expect(computeServerBusyBarCount(0)).toBe(0);
    expect(computeServerBusyBarCount(-40)).toBe(0);
    expect(computeServerBusyBarCount(Number.NaN)).toBe(0);
  });

  it('fills the width with 3px bars and 2px gaps without overflowing', () => {
    // N bars span 5N-2 px; the count is floor((width + 2) / 5) and never exceeds the width.
    expect(computeServerBusyBarCount(3)).toBe(1);
    expect(computeServerBusyBarCount(7)).toBe(1);
    expect(computeServerBusyBarCount(8)).toBe(2);
    expect(computeServerBusyBarCount(100)).toBe(20);
    expect(computeServerBusyBarCount(1920)).toBe(384);
    for (const width of [1, 2, 5, 13, 101, 777, 1920]) {
      const count = computeServerBusyBarCount(width);
      if (count > 0) expect(count * 3 + (count - 1) * 2).toBeLessThanOrEqual(width);
    }
  });
});

describe('server busy state', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    // Drain any outstanding requests and settle the linger timer between tests.
    while (serverInFlightCount() > 0) endServerRequest();
    vi.runOnlyPendingTimers();
    vi.useRealTimers();
    serverBusy.value = false;
  });

  it('is busy while any request is in flight and lingers briefly after the last one settles', () => {
    expect(serverBusy.value).toBe(false);
    beginServerRequest();
    expect(serverBusy.value).toBe(true);
    beginServerRequest();
    endServerRequest();
    // Still one in flight → still busy, no linger timer engaged yet.
    expect(serverInFlightCount()).toBe(1);
    expect(serverBusy.value).toBe(true);
    endServerRequest();
    // Idle now, but the indicator lingers to avoid flicker across rapid bursts.
    expect(serverInFlightCount()).toBe(0);
    expect(serverBusy.value).toBe(true);
    vi.advanceTimersByTime(320);
    expect(serverBusy.value).toBe(false);
  });

  it('cancels the pending hide when a new request starts during the linger', () => {
    beginServerRequest();
    endServerRequest();
    expect(serverBusy.value).toBe(true);
    vi.advanceTimersByTime(100);
    beginServerRequest();
    vi.advanceTimersByTime(400);
    // A request started mid-linger keeps it visible.
    expect(serverBusy.value).toBe(true);
    endServerRequest();
    vi.advanceTimersByTime(320);
    expect(serverBusy.value).toBe(false);
  });
});

describe('ServerBusyBars', () => {
  it('draws the requested number of inert, staggered bars and reflects the busy flag', () => {
    const markup = String(ServerBusyBars({ count: 4, busy: true }));
    expect(markup).toContain('data-component="server-busy-bars"');
    expect(markup).toContain('aria-hidden="true"');
    expect(markup).toContain('data-visible="true"');
    expect((markup.match(/server-busy-bars__bar/g) ?? []).length).toBe(4);
    expect(markup).toContain('--bar-index:0');
    expect(markup).toContain('--bar-index:3');
    const hidden = String(ServerBusyBars({ count: 2, busy: false }));
    expect(hidden).toContain('data-visible="false"');
    expect((hidden.match(/server-busy-bars__bar/g) ?? []).length).toBe(2);
    expect(String(ServerBusyBars({ count: 0, busy: false }))).not.toContain('server-busy-bars__bar');
  });

  it('uses the star yellow token, pins to the top, allocates no space, and respects reduced motion', () => {
    const css = readFileSync(new URL('./components/server-busy-bars.css', import.meta.url), 'utf8');
    expect(css).toContain('background: var(--hs-ticket-state-up-next)');
    expect(css).toContain('position: fixed');
    expect(css).toContain('inset: 0 0 auto 0');
    expect(css).toContain('height: 4px');
    expect(css).toContain('pointer-events: none');
    expect(css).toMatch(/\.server-busy-bars__bar \{[^}]*flex: 0 0 3px/);
    expect(css).toMatch(/\.server-busy-bars \{[^}]*gap: 2px/);
    expect(css).toContain('animation-play-state: paused');
    expect(css).toContain('@media (prefers-reduced-motion: reduce)');
  });
});
