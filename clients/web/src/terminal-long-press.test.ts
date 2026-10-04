import { describe, expect, it, vi } from 'vitest';

import {
  createLongPressController,
  type LongPressPoint,
  TERMINAL_LONG_PRESS_MS,
  TERMINAL_LONG_PRESS_TOLERANCE,
} from './terminal-long-press';

function harness() {
  const timers: Array<{ callback: () => void; delay: number; cancelled: boolean }> = [],
    presses: LongPressPoint[] = [];
  const controller = createLongPressController({
    onLongPress: (point) => presses.push(point),
    schedule: (callback, delay) => {
      const timer = { callback, delay, cancelled: false };
      timers.push(timer);
      return () => {
        timer.cancelled = true;
      };
    },
  });
  /** Let the newest pending hold timer elapse. */
  const elapse = () => {
    const timer = timers.at(-1);
    if (timer && !timer.cancelled) timer.callback();
  };
  return { controller, timers, presses, elapse };
}

describe('terminal long-press controller (HS2-KKP8YJ)', () => {
  it('fires once at the touch-down point after the hold and reports the fired lift', () => {
    const { controller, timers, presses, elapse } = harness();
    expect(controller.origin).toBeUndefined();
    controller.start({ x: 40, y: 80 });
    expect(timers[0].delay).toBe(TERMINAL_LONG_PRESS_MS);
    expect(controller.active).toBe(true);
    // The runtime reads the touch-down point at the lift to place the edit menu (HS2-EYR96N).
    expect(controller.origin).toEqual({ x: 40, y: 80 });
    expect(controller.fired).toBe(false);
    // Drift inside the tolerance keeps the press alive and the original point.
    controller.move({ x: 40 + TERMINAL_LONG_PRESS_TOLERANCE, y: 80 - TERMINAL_LONG_PRESS_TOLERANCE });
    elapse();
    expect(presses).toEqual([{ x: 40, y: 80 }]);
    expect(controller.fired).toBe(true);
    // Movement after firing neither cancels nor refires.
    controller.move({ x: 200, y: 300 });
    expect(controller.fired).toBe(true);
    expect(controller.origin).toEqual({ x: 40, y: 80 });
    expect(controller.end()).toBe(true);
    expect(controller.active).toBe(false);
    expect(controller.fired).toBe(false);
    expect(controller.origin).toBeUndefined();
    expect(presses).toHaveLength(1);
  });

  it('treats an early lift as a tap and cancels its timer', () => {
    const { controller, timers, presses, elapse } = harness();
    controller.start({ x: 1, y: 1 });
    expect(controller.end()).toBe(false);
    expect(timers[0].cancelled).toBe(true);
    elapse();
    expect(presses).toEqual([]);
  });

  it('abandons the press when the finger drifts past the tolerance on either axis', () => {
    for (const drift of [
      { x: TERMINAL_LONG_PRESS_TOLERANCE + 1, y: 0 },
      { x: 0, y: -(TERMINAL_LONG_PRESS_TOLERANCE + 1) },
    ]) {
      const { controller, timers, presses, elapse } = harness();
      controller.start({ x: 50, y: 50 });
      controller.move({ x: 50 + drift.x, y: 50 + drift.y });
      expect(timers[0].cancelled).toBe(true);
      expect(controller.active).toBe(false);
      // Coming back does not resurrect it; only a new touch starts a press.
      controller.move({ x: 50, y: 50 });
      elapse();
      expect(presses).toEqual([]);
      expect(controller.end()).toBe(false);
    }
  });

  it('cancels for a second finger, scroll, or touchcancel, and a later touch starts fresh', () => {
    const { controller, timers, presses, elapse } = harness();
    controller.start({ x: 5, y: 5 });
    controller.cancel();
    expect(timers[0].cancelled).toBe(true);
    expect(controller.end()).toBe(false);
    // Cancelling after a fire clears the fired lift too.
    controller.start({ x: 6, y: 6 });
    elapse();
    expect(controller.fired).toBe(true);
    controller.cancel();
    expect(controller.fired).toBe(false);
    expect(controller.end()).toBe(false);
    // Empty-then-refill: a fresh touch after cancellation fires normally.
    controller.start({ x: 7, y: 7 });
    elapse();
    expect(presses).toEqual([
      { x: 6, y: 6 },
      { x: 7, y: 7 },
    ]);
  });

  it('restarts on a repeated touch-down without letting the stale timer fire', () => {
    const { controller, timers, presses } = harness();
    controller.start({ x: 1, y: 1 });
    controller.start({ x: 9, y: 9 });
    expect(timers[0].cancelled).toBe(true);
    expect(timers[1].cancelled).toBe(false);
    timers[1].callback();
    expect(presses).toEqual([{ x: 9, y: 9 }]);
    expect(controller.end()).toBe(true);
  });

  it('ignores moves and ends with no touch down and supports custom timing', () => {
    const schedule = vi.fn(() => () => undefined),
      controller = createLongPressController({ onLongPress: vi.fn(), schedule, delay: 300, tolerance: 2 });
    controller.move({ x: 0, y: 0 });
    expect(controller.end()).toBe(false);
    controller.cancel();
    controller.start({ x: 0, y: 0 });
    expect(schedule).toHaveBeenCalledWith(expect.any(Function), 300);
    controller.move({ x: 3, y: 0 });
    expect(controller.active).toBe(false);
  });
});
