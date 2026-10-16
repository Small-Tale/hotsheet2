import './toast-clearance.css';

/** The custom property `.app-toast-stack` lifts its bottom edge by (`toast-clearance.css`). */
export const TOAST_CLEARANCE_PROPERTY = '--app-toast-clearance';

/** The parts of the toast stack host and of an obstacle element the clearance reads or writes. */
export interface ToastClearanceHost {
  readonly style: Pick<CSSStyleDeclaration, 'setProperty' | 'removeProperty'>;
}
export interface ToastObstacle {
  readonly isConnected: boolean;
  getBoundingClientRect(): { readonly top: number; readonly height: number };
}

export interface ToastClearanceEnvironment<Obstacle extends ToastObstacle = Element> {
  /** The bottom-anchored surface a toast must stay above while it is on screen, if any. */
  findObstacle: () => Obstacle | null;
  viewportHeight: () => number;
  observeResize: (callback: () => void) => { observe(target: Obstacle): void; disconnect(): void };
}

const browserEnvironment = (findObstacle: () => Element | null): ToastClearanceEnvironment => ({
  findObstacle,
  viewportHeight: () => window.innerHeight,
  observeResize: (callback) => new ResizeObserver(callback),
});

/**
 * Keep the bottom-placed toast stack clear of a full-width bottom control strip, such as the media
 * gallery's footer with its zoom and markup toolbars (HS2-DSW9TY). While a toast shows, the stack's
 * bottom edge follows the measured top of that strip — including when the strip changes height, as it
 * does when markup finishes — and returns to the viewport edge once the strip is gone.
 */
export function createToastClearance<Obstacle extends ToastObstacle = Element>(
  host: ToastClearanceHost,
  environment: ToastClearanceEnvironment<Obstacle>,
) {
  let target: Obstacle | undefined;
  let observer: ReturnType<ToastClearanceEnvironment<Obstacle>['observeResize']> | undefined;
  const apply = () => {
    const rect = target?.isConnected ? target.getBoundingClientRect() : undefined;
    const clearance = rect && rect.height > 0 ? Math.max(0, Math.round(environment.viewportHeight() - rect.top)) : 0;
    if (clearance > 0) host.style.setProperty(TOAST_CLEARANCE_PROPERTY, `${clearance}px`);
    else host.style.removeProperty(TOAST_CLEARANCE_PROPERTY);
  };
  const stop = () => {
    observer?.disconnect();
    observer = undefined;
    target = undefined;
    host.style.removeProperty(TOAST_CLEARANCE_PROPERTY);
  };
  return {
    /** A toast is (still) showing: find the current obstacle, follow its size, and clear it now. */
    track() {
      const next = environment.findObstacle() ?? undefined;
      if (next !== target) {
        observer?.disconnect();
        observer = undefined;
        target = next;
        if (target) {
          observer = environment.observeResize(apply);
          observer.observe(target);
        }
      }
      apply();
    },
    /** No toast shows: stop observing and return the stack to the viewport edge. */
    stop,
  };
}

/** The browser form: measure `findObstacle()` against the window and follow it with a ResizeObserver. */
export function createBrowserToastClearance(host: ToastClearanceHost, findObstacle: () => Element | null) {
  return createToastClearance(host, browserEnvironment(findObstacle));
}
