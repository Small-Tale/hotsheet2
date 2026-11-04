/**
 * Which viewer answers the program's terminal queries (HS2-0GZZHV).
 *
 * Every xterm attached to a PTY answers the program's queries (cursor position, device
 * attributes, mode and colour reports) and reports focus changes through `onData`, alongside
 * what the user types. With several viewers attached, the program would read one reply per
 * viewer, and a passive viewer's cursor-position reply describes its own grid rather than the
 * PTY's. The server names one answerer in every size frame (`answered_by`): the driver, or the
 * earliest-attached viewer that can answer when nobody drives. Only that viewer forwards the
 * emulator's own data; typed input from every viewer is unchanged.
 */
import type { TerminalSizeMessage } from './terminal-viewport';

/**
 * True when this viewer should forward the emulator's replies. It fails open, so a lone viewer
 * is never silent: before any size frame, or while the server names no answerer (no viewer
 * that can answer has claimed yet), the viewer answers. A server that predates `answered_by`
 * lets its driver answer, and every viewer answer an undriven terminal, as before.
 */
export function terminalAnswersQueries(viewerId: string, size: TerminalSizeMessage | undefined): boolean {
  if (!size) return true;
  if (typeof size.answered_by === 'string') return size.answered_by === viewerId;
  if (size.answered_by === null) return true;
  return typeof size.driven_by === 'string' ? size.driven_by === viewerId : true;
}

/** Tells whether each `onData` datum was user input (keys, IME composition, paste, mouse). */
export interface TerminalUserInputTracker {
  /** False when the emulator does not expose its user-input signal; then all data is typed input. */
  readonly supported: boolean;
  /** Call once per `onData` datum: whether that datum was user input. */
  wasUserInput(): boolean;
  dispose(): void;
}

interface UserInputSignal {
  onUserInput(listener: () => void): { dispose(): void };
}

/**
 * xterm labels each datum itself: `CoreService.triggerDataEvent(data, wasUserInput)` fires
 * `onUserInput` synchronously just before `onData` for keyboard, composition, paste, and mouse
 * input, and not for parser replies or focus reports. xterm does not expose that label publicly,
 * so this reads it from the one internal service, pinned by a test against the installed xterm.
 * DOM event timing cannot replace it: xterm emits IME composition input from a later timer, so a
 * phone that is not the driver would lose typed characters.
 */
function userInputSignal(terminal: object): UserInputSignal | undefined {
  const service = (terminal as { _core?: { coreService?: Partial<UserInputSignal> } })._core?.coreService;
  return typeof service?.onUserInput === 'function' ? (service as UserInputSignal) : undefined;
}

/**
 * Follow xterm's user-input label. Without it every datum counts as typed input, so a future
 * xterm can at worst bring back duplicate replies; it can never drop what the user typed.
 */
export function trackTerminalUserInput(terminal: object): TerminalUserInputTracker {
  const signal = userInputSignal(terminal);
  if (!signal) return { supported: false, wasUserInput: () => true, dispose: () => undefined };
  let pending = false;
  const subscription = signal.onUserInput(() => {
    pending = true;
  });
  return {
    supported: true,
    wasUserInput: () => {
      const was = pending;
      pending = false;
      return was;
    },
    dispose: () => {
      subscription.dispose();
    },
  };
}
