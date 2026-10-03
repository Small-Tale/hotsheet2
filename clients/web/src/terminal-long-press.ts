/**
 * Long-press on a touch terminal (HS2-KKP8YJ).
 *
 * Holding one finger still on an interactive terminal for {@link TERMINAL_LONG_PRESS_MS} opens the
 * terminal edit menu (Copy Text… / Paste) at the touch point, the conventional mobile edit gesture.
 * It coexists with the finger-drag scroll controller (`terminal-touch-scroll.ts`): any movement past
 * the tolerance, a second finger, or lifting early cancels the press, so a drag scrolls and a quick
 * tap still focuses the terminal.
 */

/** Hold time before a still touch counts as a long press. */
export const TERMINAL_LONG_PRESS_MS = 500;
/** Movement (CSS px, either axis) a held finger may drift before the press is abandoned. */
export const TERMINAL_LONG_PRESS_TOLERANCE = 10;

export interface LongPressPoint {
  x: number;
  y: number;
}

export interface LongPressOptions {
  /** Called once when the press completes, with the point where the finger went down. */
  onLongPress: (point: LongPressPoint) => void;
  /** Schedule the hold timer; returns a cancel function. Local UI timing only, never network. */
  schedule: (callback: () => void, delay: number) => () => void;
  delay?: number;
  tolerance?: number;
}

export interface LongPressController {
  /** A single finger touched down. Restarts any earlier press. */
  start(point: LongPressPoint): void;
  /** The finger moved; drifting past the tolerance abandons the press. */
  move(point: LongPressPoint): void;
  /** The finger lifted. Returns whether this touch fired a long press (the caller then suppresses the tap). */
  end(): boolean;
  /** Abandon the press (second finger, scroll, touchcancel, teardown). */
  cancel(): void;
  /** Whether a touch is currently held (pending or fired). */
  readonly active: boolean;
  /** Whether the current touch has fired. */
  readonly fired: boolean;
}

export function createLongPressController({
  onLongPress,
  schedule,
  delay = TERMINAL_LONG_PRESS_MS,
  tolerance = TERMINAL_LONG_PRESS_TOLERANCE,
}: LongPressOptions): LongPressController {
  let origin: LongPressPoint | undefined,
    fired = false,
    cancelTimer: (() => void) | undefined;
  const stopTimer = () => {
    cancelTimer?.();
    cancelTimer = undefined;
  };
  return {
    start(point) {
      stopTimer();
      origin = point;
      fired = false;
      cancelTimer = schedule(() => {
        cancelTimer = undefined;
        if (!origin) return;
        fired = true;
        onLongPress(origin);
      }, delay);
    },
    move(point) {
      if (!origin || fired) return;
      if (Math.abs(point.x - origin.x) > tolerance || Math.abs(point.y - origin.y) > tolerance) {
        stopTimer();
        origin = undefined;
      }
    },
    end() {
      const result = fired;
      stopTimer();
      origin = undefined;
      fired = false;
      return result;
    },
    cancel() {
      stopTimer();
      origin = undefined;
      fired = false;
    },
    get active() {
      return origin !== undefined;
    },
    get fired() {
      return fired;
    },
  };
}
