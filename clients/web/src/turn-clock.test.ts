import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { createTurnClock } from './turn-clock';

describe('turn clock (HS2-XV1VCR)', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it('ticks only while a turn runs, never twice per interval, and stops itself', () => {
    let running = false;
    const tick = vi.fn(),
      clock = createTurnClock(() => running, tick, 1_000);
    clock.sync();
    vi.advanceTimersByTime(5_000);
    expect(tick).not.toHaveBeenCalled();
    expect(clock.running()).toBe(false);

    running = true;
    clock.sync();
    clock.sync(); // repeated sync keeps one timer
    vi.advanceTimersByTime(3_000);
    expect(tick).toHaveBeenCalledTimes(3);

    // The turn ends without an explicit sync: the next interval stops the clock.
    running = false;
    vi.advanceTimersByTime(1_000);
    expect(tick).toHaveBeenCalledTimes(3);
    expect(clock.running()).toBe(false);

    // Empty then refill.
    running = true;
    clock.sync();
    vi.advanceTimersByTime(1_000);
    expect(tick).toHaveBeenCalledTimes(4);
    running = false;
    clock.sync();
    expect(clock.running()).toBe(false);
    clock.stop();
    vi.advanceTimersByTime(10_000);
    expect(tick).toHaveBeenCalledTimes(4);
  });
});
