/** One application toast at a time; Web Awesome owns presentation and manual dismissal. */
export interface ToastState {
  message: string;
  generation: number;
}

export interface ToastClock {
  setTimeout(callback: () => void, delay: number): number;
  clearTimeout(id: number): void;
}

export function createToastLifetime(
  publish: (state: ToastState) => void,
  clock: ToastClock = globalThis,
  duration = 2_500,
) {
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
    show(message: string) {
      clear();
      state = { message, generation: state.generation + 1 };
      publish(state);
      const generation = state.generation;
      timeout = clock.setTimeout(() => {
        hide(generation);
      }, duration);
    },
    hide,
    dispose: clear,
  };
}
