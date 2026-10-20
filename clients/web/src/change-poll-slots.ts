/** Cross-context direct-poll slots for bridges without change multiplexing. */
const SLOT_COUNT = 4;
const FALLBACK_SPACING_MS = 250;

const aborted = () => new DOMException('The change poll was aborted.', 'AbortError');

function wait(milliseconds: number, signal: AbortSignal): Promise<void> {
  if (signal.aborted) return Promise.reject(aborted());
  if (milliseconds <= 0) return Promise.resolve();
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      signal.removeEventListener('abort', onAbort);
      resolve();
    }, milliseconds);
    const onAbort = () => {
      clearTimeout(timer);
      reject(aborted());
    };
    signal.addEventListener('abort', onAbort, { once: true });
  });
}

export class DirectPollSlots {
  private nextSlot = 0;
  private nextFallbackStart = 0;

  async run<T>(signal: AbortSignal, poll: (timeoutMs: number) => Promise<T>): Promise<T> {
    if (signal.aborted) throw aborted();
    const host = Reflect.get(globalThis, 'navigator') as Navigator | undefined;
    const locks = host?.locks;
    if (locks) {
      const slot = this.nextSlot++ % SLOT_COUNT;
      const state = { acquired: false };
      try {
        return await locks.request(`hotsheet-direct-change-poll-${slot}`, { mode: 'exclusive', signal }, () => {
          state.acquired = true;
          return poll(1_000);
        });
      } catch (reason) {
        if (state.acquired || (reason instanceof Error && reason.name === 'AbortError')) throw reason;
        // If a Web Locks implementation refuses this context, use short requests instead.
      }
    }
    // With no cross-context lock, never hold a connection through a long poll.
    // Pacing avoids a busy loop when an older bridge returns an empty page immediately.
    const start = Math.max(Date.now(), this.nextFallbackStart);
    this.nextFallbackStart = start + FALLBACK_SPACING_MS;
    await wait(start - Date.now(), signal);
    return poll(0);
  }
}
