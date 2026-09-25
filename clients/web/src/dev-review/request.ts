/**
 * Dev Review is opt-in in development (HS2-TCACFR): `?dev-review`, `?dev-review=1`, or any value other than
 * `false`/`0` enables it; without the parameter the overlay and automatic stability reporting stay off.
 * Production builds never enable it.
 */
export function devReviewRequested(url: string, development: boolean): boolean {
  const value = new URL(url).searchParams.get('dev-review');
  return development && value !== null && value !== 'false' && value !== '0';
}

export interface DevReviewPopover {
  matches(selector: string): boolean;
  showPopover?: () => void;
  hidePopover?: () => void;
}

export function promoteDevReviewPopover(popover: DevReviewPopover): void {
  if (!popover.showPopover) return;
  if (popover.matches(':popover-open')) popover.hidePopover?.();
  popover.showPopover();
}
