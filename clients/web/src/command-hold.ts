/** How long a command button is held before it opens the command's run history instead of running. */
export const COMMAND_HOLD_MS = 550;

/** The timer surface the hold uses; the page passes `window.setTimeout` and `window.clearTimeout`. */
export interface CommandHoldClock {
  readonly setTimeout: (callback: () => void, ms: number) => number;
  readonly clearTimeout: (id: number) => void;
}

/** One press-and-hold gesture on the command buttons. */
export interface CommandHold {
  /** A pointer went down on a command button; `at` is the event's `timeStamp`. */
  press(commandId: string, at: number): void;
  /** The pointer came up; `at` is the event's `timeStamp`. Returns whether the press was a hold. */
  release(at: number): boolean;
  /** The pointer was cancelled (for example by a scroll); the press is neither a hold nor a click. */
  cancel(): void;
}

/**
 * Detect a press-and-hold on a command button.
 *
 * The timer opens the history while the button is still held. It is not the only judge, because a
 * busy main thread runs the timer late, and input is dispatched before a due timer: a release that
 * arrives with the timer still pending would cancel a hold the user really made, and the history
 * would never open (HS2-1TXYXS). The release therefore also compares the two event timestamps, which
 * record when the input happened rather than when the page got to it.
 */
export function createCommandHold(clock: CommandHoldClock, onHold: (commandId: string) => void): CommandHold {
  let pending: { commandId: string; pressedAt: number; timer: number } | undefined;
  const clear = () => {
    if (pending) clock.clearTimeout(pending.timer);
    const was = pending;
    pending = undefined;
    return was;
  };
  return {
    press(commandId, at) {
      clear();
      const timer = clock.setTimeout(() => {
        pending = undefined;
        onHold(commandId);
      }, COMMAND_HOLD_MS);
      pending = { commandId, pressedAt: at, timer };
    },
    release(at) {
      const was = clear();
      if (!was || at - was.pressedAt < COMMAND_HOLD_MS) return false;
      onHold(was.commandId);
      return true;
    },
    cancel() {
      clear();
    },
  };
}
