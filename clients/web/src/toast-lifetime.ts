/** One application toast at a time; Web Awesome owns presentation and manual dismissal. */
export interface ToastState {
  message: string;
  generation: number;
}

export interface ToastClock {
  setTimeout(callback: () => void, delay: number): number;
  clearTimeout(id: number): void;
}

/** The shortest toast lifetime: every short acknowledgement keeps it (HS2-PC7VRA). */
export const TOAST_MIN_MS = 2_500;
/** Reading time a toast gets per word of its message (GH-5, HS2-PC7VRA). */
export const TOAST_MS_PER_WORD = 300;
/** The longest automatic lifetime; any toast can still be dismissed sooner (GH-5, HS2-PC7VRA). */
export const TOAST_MAX_MS = 20_000;
/** A lifetime that never hides the toast on its own; it stays until dismissed or replaced. */
export const TOAST_PERSISTENT = Number.POSITIVE_INFINITY;

/**
 * How long a toast stays up when its caller does not choose: reading time for its words, never
 * shorter than the 2.5-second floor or longer than 20 seconds. One central rule, so a long error or
 * explanation is readable without each call opting in (HS2-PC7VRA).
 */
export function toastReadingTime(message: string): number {
  const words = message.split(/\s+/).filter(Boolean).length;
  return Math.min(TOAST_MAX_MS, Math.max(TOAST_MIN_MS, words * TOAST_MS_PER_WORD));
}

export function createToastLifetime(publish: (state: ToastState) => void, clock: ToastClock = globalThis) {
  let state: ToastState = { message: '', generation: 0 };
  let timeout: number | undefined;
  const clear = () => {
    if (timeout !== undefined) clock.clearTimeout(timeout);
    timeout = undefined;
  };
  const hide = (generation: number) => {
    if (generation !== state.generation || !state.message) return;
    clear();
    state = { ...state, message: '' };
    publish(state);
  };
  return {
    /**
     * Show `message` for its reading time, or for an explicit `lifetime` in ms. A lifetime that is
     * not a positive finite number ({@link TOAST_PERSISTENT}) keeps the toast until it is dismissed
     * or replaced.
     */
    show(message: string, lifetime = toastReadingTime(message)) {
      clear();
      state = { message, generation: state.generation + 1 };
      publish(state);
      if (!Number.isFinite(lifetime) || lifetime <= 0) return;
      const generation = state.generation;
      timeout = clock.setTimeout(() => {
        hide(generation);
      }, lifetime);
    },
    hide,
    dispose: clear,
  };
}
