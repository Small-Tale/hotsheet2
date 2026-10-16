/**
 * The local render-only clock behind running-turn progress (HS2-XV1VCR). While any AI turn runs
 * it calls `tick` once per interval so elapsed and last-activity times re-render; it stops as soon
 * as nothing runs. It never issues a network request: turn timing arrives on the change stream.
 */
export function createTurnClock(isRunning: () => boolean, tick: () => void, intervalMs = 1_000) {
  let timer: ReturnType<typeof setInterval> | undefined;
  const stop = () => {
    if (timer === undefined) return;
    clearInterval(timer);
    timer = undefined;
  };
  /** Start or stop the clock to match whether a turn is running now. */
  const sync = () => {
    if (!isRunning()) stop();
    else
      timer ??= setInterval(() => {
        if (isRunning()) tick();
        else stop();
      }, intervalMs);
  };
  return { sync, stop, running: () => timer !== undefined };
}
