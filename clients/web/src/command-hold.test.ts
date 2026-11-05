import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { COMMAND_HOLD_MS, createCommandHold } from './command-hold';

describe('createCommandHold (HS2-1TXYXS)', () => {
  let held: string[];
  const hold = () =>
    createCommandHold(
      {
        setTimeout: (callback, ms) => setTimeout(callback, ms) as unknown as number,
        clearTimeout: (id) => {
          clearTimeout(id);
        },
      },
      (commandId) => held.push(commandId),
    );
  beforeEach(() => {
    vi.useFakeTimers();
    held = [];
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it('opens the history from the timer while the button is still held', () => {
    const gesture = hold();
    gesture.press('checks', 1000);
    vi.advanceTimersByTime(COMMAND_HOLD_MS - 1);
    expect(held).toEqual([]);
    vi.advanceTimersByTime(1);
    expect(held).toEqual(['checks']);
    // The release after the timer fired is not a second hold.
    expect(gesture.release(1000 + COMMAND_HOLD_MS + 50)).toBe(false);
    expect(held).toEqual(['checks']);
  });

  it('treats a short press as a click', () => {
    const gesture = hold();
    gesture.press('checks', 1000);
    expect(gesture.release(1000 + COMMAND_HOLD_MS - 1)).toBe(false);
    vi.advanceTimersByTime(COMMAND_HOLD_MS * 2);
    expect(held).toEqual([]);
  });

  it('opens the history when a busy page delivers the release before the late timer', () => {
    const gesture = hold();
    gesture.press('checks', 1000);
    // The timer has not run yet, but the input happened a full hold after the press.
    expect(gesture.release(1000 + COMMAND_HOLD_MS)).toBe(true);
    expect(held).toEqual(['checks']);
    vi.advanceTimersByTime(COMMAND_HOLD_MS * 2);
    expect(held).toEqual(['checks']);
  });

  it('cancels a press without opening the history', () => {
    const gesture = hold();
    gesture.press('checks', 1000);
    gesture.cancel();
    vi.advanceTimersByTime(COMMAND_HOLD_MS * 2);
    expect(gesture.release(1000 + COMMAND_HOLD_MS * 2)).toBe(false);
    expect(held).toEqual([]);
  });

  it('keeps only the latest press and ignores a release with no press', () => {
    const gesture = hold();
    expect(gesture.release(5000)).toBe(false);
    gesture.press('checks', 1000);
    vi.advanceTimersByTime(COMMAND_HOLD_MS - 100);
    gesture.press('build', 1000 + COMMAND_HOLD_MS - 100);
    vi.advanceTimersByTime(100);
    expect(held).toEqual([]);
    expect(gesture.release(1000 + COMMAND_HOLD_MS * 2)).toBe(true);
    expect(held).toEqual(['build']);
    // Empty, then a fresh hold works again.
    gesture.press('checks', 9000);
    vi.advanceTimersByTime(COMMAND_HOLD_MS);
    expect(held).toEqual(['build', 'checks']);
  });
});
