/**
 * The terminal drawer rail's search match count yields its room at narrow drawer widths (HS2-HPDX81).
 *
 * Kerf's TabBar never shrinks its `end` zone. With the search open, that zone holds the field, the
 * match count control (HS2-M5QPED), and Clear and Hide. On a phone-width drawer the zone is wider
 * than the rail: the tabs collapse and then the count, Clear, and Hide are pushed past the drawer's
 * edge. On a phone-width rail (below 30rem, where terminal-drawer.css also narrows the field), and
 * whenever a wider rail's end still overflows, the drawer shows the count compactly instead. The visible
 * control leaves the rail, and the count stays available as a visually hidden polite status. Enter and
 * Shift-Enter in the field still step through the matches.
 *
 * The decision uses only app-owned geometry: the `.terminal-drawer__views` wrapper and the app's own
 * Hide button. It never reads Kerf's parts. Kerf's `onFitChange` cannot be used because its reports
 * carry only the bar id, and every catalog drawer specimen shares that id. Toolbar `hideBelow` and
 * `visibility` work only inside a Toolbar, not a TabBar end zone.
 */
import { TERMINALS_ACTIONS } from './interaction-attrs/terminals';
import { TERMINAL_SEARCH_FIELD_ID } from './terminal-search';

/** One measurement of the drawer rail. */
export interface TerminalRailMeasure {
  /** The app-owned rail wrapper's inline width. */
  railWidth: number;
  /** At or below this rail width the count is always compact: the phone-width rail of terminal-drawer.css. */
  narrowWidth: number;
  /** How far the Hide button reaches past the wrapper's content box; 0 or less when it fits. */
  endOverflow: number;
  /** The full match count control is on the rail. */
  countShown: boolean;
}

/** Hysteresis memory: the rail width the full count needed when it last overflowed. */
export interface TerminalRailMemory {
  fullWidth?: number;
}

/**
 * Whether the count should be compact after this measurement. A phone-width rail is always compact. A
 * wider rail turns compact when its end overflows while the full count shows, and remembers the width
 * that layout needed; it turns full again only once the rail is that wide, so a rail that fits only
 * because the count is compact never flips back and forth.
 */
export function terminalSearchCountCompact(
  compact: boolean,
  measure: TerminalRailMeasure,
  memory: TerminalRailMemory,
): boolean {
  if (measure.railWidth <= measure.narrowWidth) return true;
  if (!compact) {
    if (!measure.countShown || measure.endOverflow <= 0) return false;
    memory.fullWidth = measure.railWidth + measure.endOverflow;
    return true;
  }
  if (memory.fullWidth === undefined) return false;
  return measure.railWidth < memory.fullWidth;
}

const RAIL = '.terminal-drawer__views';
const FIELD = `[data-search-field-id="${TERMINAL_SEARCH_FIELD_ID}"]`;
const COUNT = '[data-terminal-search-count]';
const HIDE = TERMINALS_ACTIONS.toggleTerminalDrawer.selector;

function contentRight(element: HTMLElement) {
  const style = getComputedStyle(element);
  return element.getBoundingClientRect().right - (Number.parseFloat(style.paddingRight) || 0);
}

/** The phone-width rail threshold, 30rem as in terminal-drawer.css's container query. */
function narrowWidth() {
  return 30 * (Number.parseFloat(getComputedStyle(document.documentElement).fontSize) || 16);
}

/**
 * Follow the rail of the drawer under `root` that holds the terminal search field and publish whether its
 * match count should be compact. Only layout and DOM observers drive it: no timers and no requests.
 */
export function wireTerminalSearchCountFit(root: HTMLElement, onCompact: (compact: boolean) => void): () => void {
  let rail: HTMLElement | null = null,
    compact = false,
    frame: number | undefined;
  const memory: TerminalRailMemory = {};
  const refresh = () => {
    frame = undefined;
    if (!rail?.isConnected || rail.clientWidth === 0) return;
    const hide = rail.querySelector<HTMLElement>(HIDE);
    if (!hide) return;
    const next = terminalSearchCountCompact(
      compact,
      {
        railWidth: rail.clientWidth,
        narrowWidth: narrowWidth(),
        endOverflow: Math.round(hide.getBoundingClientRect().right - contentRight(rail)),
        countShown: rail.querySelector(COUNT) !== null,
      },
      memory,
    );
    if (next === compact) return;
    compact = next;
    onCompact(compact);
  };
  const schedule = () => {
    frame ??= requestAnimationFrame(refresh);
  };
  // The rail's width and the count control's size (its text) both move Hide.
  const resize = new ResizeObserver(schedule);
  let count: Element | null = null;
  const observeCount = () => {
    const next = rail?.querySelector(COUNT) ?? null;
    if (next === count) return;
    if (count) resize.unobserve(count);
    count = next;
    if (count) resize.observe(count);
  };
  // The count appears, changes, and leaves through renders of the rail's own subtree.
  const railMutations = new MutationObserver(() => {
    observeCount();
    schedule();
  });
  const observeRail = () => {
    const next =
      root.querySelector(FIELD)?.closest<HTMLElement>(RAIL) ??
      // A drawer without a selected terminal has no search field; keep following the last rail.
      (rail?.isConnected ? rail : null);
    if (next === rail) return;
    resize.disconnect();
    railMutations.disconnect();
    rail = next;
    count = null;
    if (rail) {
      resize.observe(rail);
      railMutations.observe(rail, { childList: true, subtree: true, characterData: true });
      observeCount();
      schedule();
    } else if (compact) {
      // The drawer closed: the next rail starts from the full count.
      delete memory.fullWidth;
      compact = false;
      onCompact(false);
    }
  };
  // Only a render outside the rail can replace it; the check is a cheap lookup, never a layout read.
  const rootMutations = new MutationObserver(observeRail);
  rootMutations.observe(root, { childList: true, subtree: true });
  observeRail();
  return () => {
    rootMutations.disconnect();
    railMutations.disconnect();
    resize.disconnect();
    if (frame !== undefined) cancelAnimationFrame(frame);
  };
}
