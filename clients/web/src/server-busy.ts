import { signal } from 'kerfjs';

/**
 * Global "the server is doing something" state, derived from the number of in-flight
 * authenticated server requests. A short idle linger keeps the indicator from flickering
 * across rapid request bursts (e.g. debounced autosaves). Consumed by {@link ServerBusyBars}.
 */
const inFlight = signal(0);
export const serverBusy = signal(false);
/**
 * Human-readable description of what the server is most recently doing (e.g. "Loading tickets"),
 * for the optional loading-activity label. Empty when idle. Shows the latest-started operation.
 */
export const serverBusyMessage = signal('');

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
      if (inFlight.value === 0) {
        serverBusy.value = false;
        serverBusyMessage.value = '';
      }
    }, IDLE_LINGER_MS);
  }
}

/**
 * Mark one server request as started. Pair every call with exactly one {@link endServerRequest}.
 * An optional human-readable `label` becomes the current {@link serverBusyMessage}.
 */
export function beginServerRequest(label?: string): void {
  inFlight.value += 1;
  if (label) serverBusyMessage.value = label;
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
 * A short human-readable description of a server request, derived from its method and path, for
 * the optional loading-activity label. Deliberately coarse — it names the kind of work, not the
 * exact endpoint — and falls back to a generic phrase for unrecognized paths.
 */
export function describeServerRequest(method: string, path: string): string {
  const verb = method.toUpperCase();
  const writing = verb === 'POST' || verb === 'PUT' || verb === 'PATCH' || verb === 'DELETE';
  const has = (segment: string) => path.includes(segment);
  if (has('/search')) return 'Searching tickets';
  if (has('/attachments') || has('/thumbnail') || has('/media')) return writing ? 'Uploading attachment' : 'Loading attachment';
  if (has('/notes')) return 'Saving note';
  if (has('/code-review')) return 'Loading code review';
  if (has('/repository') || has('/git')) return writing ? 'Updating repository' : 'Checking repository';
  if (has('/turn') || has('/drive') || has('/conversation') || has('/tool')) return 'Talking to the AI tool';
  if (has('/terminals') || has('/terminal')) return 'Preparing terminals';
  if (has('/commands') || has('/command')) return writing ? 'Running command' : 'Loading commands';
  if (has('/providers') || has('/models') || has('/ai')) return 'Loading AI tools';
  if (has('/permission')) return 'Updating permissions';
  if (has('/setup') || has('/projects/open') || has('/bootstrap')) return 'Preparing project';
  if (has('/tickets') || has('/checkout')) return writing ? 'Saving ticket' : 'Loading tickets';
  return writing ? 'Saving changes' : 'Loading…';
}

/**
 * Number of 3px bars (separated by 2px gaps) that fit across `width` pixels, so the decorative
 * top overlay fills the viewport width exactly. `N` bars span `5N - 2` px, so `N = floor((width + 2) / 5)`.
 */
export function computeServerBusyBarCount(width: number, barWidth = 3, gap = 2): number {
  if (!Number.isFinite(width) || width <= 0) return 0;
  return Math.floor((width + gap) / (barWidth + gap));
}
