import { signal } from 'kerfjs';

/**
 * Global "the server is doing something" state, derived from the number of in-flight
 * authenticated server requests. A short idle linger keeps the indicator from flickering
 * across rapid request bursts (e.g. debounced autosaves). Consumed by {@link ServerBusyBars}.
 */
const inFlight = signal(0);
export const serverBusy = signal(false);

const IDLE_LINGER_MS = 300;
let idleTimer: ReturnType<typeof setTimeout> | undefined;

function sync(): void {
  if (inFlight.value > 0) {
    if (idleTimer !== undefined) {
      clearTimeout(idleTimer);
      idleTimer = undefined;
    }
    serverBusy.value = true;
  } else if (idleTimer === undefined && serverBusy.value) {
    idleTimer = setTimeout(() => {
      idleTimer = undefined;
      if (inFlight.value === 0) serverBusy.value = false;
    }, IDLE_LINGER_MS);
  }
}

/** Mark one server request as started. Pair every call with exactly one {@link endServerRequest}. */
export function beginServerRequest(): void {
  inFlight.value += 1;
  sync();
}

/** Mark one server request as finished (success or failure). */
export function endServerRequest(): void {
  inFlight.value = Math.max(0, inFlight.value - 1);
  sync();
}

/** Test-only helper: current in-flight request count. */
export function serverInFlightCount(): number {
  return inFlight.value;
}

/**
 * Number of 3px bars (separated by 2px gaps) that fit across `width` pixels, so the decorative
 * top overlay fills the viewport width exactly. `N` bars span `5N - 2` px, so `N = floor((width + 2) / 5)`.
 */
export function computeServerBusyBarCount(width: number, barWidth = 3, gap = 2): number {
  if (!Number.isFinite(width) || width <= 0) return 0;
  return Math.floor((width + gap) / (barWidth + gap));
}
