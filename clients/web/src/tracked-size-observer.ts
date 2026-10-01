/**
 * A ResizeObserver that follows whichever element currently fills a rendered slot (HS2-0PF13V).
 *
 * A plain observer stays bound to the node it was created with. When a re-render replaces that
 * node (a view switch unmounts and remounts the terminal drawer, for example) the observer keeps
 * watching the detached node, reports it once as 0x0, and never reports the live node's size.
 * `sync()` re-binds to the current node, so callers run it after every render; zero-size reports
 * (a hidden or detached slot) are ignored instead of being recorded as the slot's size.
 */
export interface TrackedSizeObserverOptions {
  /** The element whose size is tracked right now, or null when the slot is not rendered. */
  find: () => HTMLElement | null | undefined;
  /** Receives the floored, non-zero content size of the tracked element. */
  onSize: (size: { width: number; height: number }, target: HTMLElement) => void;
  /** Whether tracking is wanted at all; when false `sync()` releases the observer. */
  enabled?: () => boolean;
  /** Reports arriving while this is true are dropped (an owner applies the final size itself). */
  paused?: () => boolean;
  /** Injectable for tests; defaults to the browser ResizeObserver. */
  createObserver?: (callback: ResizeObserverCallback) => Pick<ResizeObserver, 'observe' | 'disconnect'>;
}

export interface TrackedSizeObserver {
  /** Bind to the slot's current element (re-binding when it was replaced or removed). */
  sync: () => void;
  /** Stop observing; a later `sync()` binds again. */
  disconnect: () => void;
  /** The element currently observed, for diagnostics and tests. */
  target: () => HTMLElement | undefined;
}

export function createTrackedSizeObserver({
  find,
  onSize,
  enabled = () => true,
  paused = () => false,
  createObserver = (callback) => new ResizeObserver(callback),
}: TrackedSizeObserverOptions): TrackedSizeObserver {
  let observer: Pick<ResizeObserver, 'observe' | 'disconnect'> | undefined, current: HTMLElement | undefined;
  const disconnect = () => {
    observer?.disconnect();
    observer = undefined;
    current = undefined;
  };
  const report: ResizeObserverCallback = (entries) => {
    if (paused()) return;
    // Only the live binding counts; a stale entry for a node already swapped out is ignored.
    const entry = [...entries].reverse().find((item) => item.target === current);
    if (!entry || !current) return;
    const width = Math.floor(entry.contentRect.width),
      height = Math.floor(entry.contentRect.height);
    if (width <= 0 || height <= 0) return;
    onSize({ width, height }, current);
  };
  const sync = () => {
    const next = enabled() ? (find() ?? undefined) : undefined;
    if (next && !next.isConnected) {
      disconnect();
      return;
    }
    if (next === current && observer) return;
    disconnect();
    if (!next) return;
    current = next;
    observer = createObserver(report);
    // Observing reports the new node's size on the next frame, so a remount re-measures at once.
    observer.observe(next);
  };
  return { sync, disconnect, target: () => current };
}
