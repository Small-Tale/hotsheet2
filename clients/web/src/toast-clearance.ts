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

// The obstacle's own size and the window's size both move its top edge: the restore button keeps its
// size but follows the viewport's bottom edge when the window resizes (HS2-T90WCZ).
const browserEnvironment = (findObstacle: () => Element | null): ToastClearanceEnvironment => ({
  findObstacle,
  viewportHeight: () => window.innerHeight,
  observeResize: (callback) => {
    const observer = new ResizeObserver(callback);
    window.addEventListener('resize', callback);
    return {
      observe: (target) => {
        observer.observe(target);
      },
      disconnect: () => {
        observer.disconnect();
        window.removeEventListener('resize', callback);
      },
    };
  },
});

/**
 * Keep the bottom-placed toast stack clear of a bottom control, such as the media gallery's footer
 * with its zoom and markup toolbars (HS2-DSW9TY) or the terminal drawer's restore button (HS2-T90WCZ). While a toast shows, the stack's
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

/** The media gallery's full-width footer strip with its zoom and markup toolbars (HS2-DSW9TY). */
export const GALLERY_FOOTER_SELECTOR = '[data-component="attachment-gallery"] .attachment-gallery__footer';
/**
 * The collapsed terminal drawer's restore toolbar in the Workbench's bottom-end corner (HS2-T90WCZ).
 * Its app-owned theme wrapper is `display: contents`, so the toolbar inside it is what takes space.
 */
export const TERMINAL_DRAWER_RESTORE_SELECTOR = '.app-shell__restore-control-theme > *';

/**
 * The bottom-anchored surface a showing toast must clear. The open media gallery's footer wins, since
 * the modal gallery covers everything beneath it; otherwise the collapsed terminal drawer's round
 * restore button, which shares the toast's bottom-end corner and would cover its first words.
 */
export function findToastObstacle<Obstacle>(root: { querySelector(selector: string): Obstacle | null }) {
  return root.querySelector(GALLERY_FOOTER_SELECTOR) ?? root.querySelector(TERMINAL_DRAWER_RESTORE_SELECTOR);
}

/** The browser form: measure `findObstacle()` against the window and follow it with a ResizeObserver. */
export function createBrowserToastClearance(host: ToastClearanceHost, findObstacle: () => Element | null) {
  return createToastClearance(host, browserEnvironment(findObstacle));
}
