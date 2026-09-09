export function devReviewRequested(url: string, development: boolean): boolean {
  return development && new URL(url).searchParams.get('dev-review') !== 'false';
}

export interface DevReviewPopover {
  matches(selector:string):boolean;
  showPopover?:()=>void;
  hidePopover?:()=>void;
}

export function promoteDevReviewPopover(popover:DevReviewPopover):void {
  if (!popover.showPopover) return;
  if (popover.matches(':popover-open')) popover.hidePopover?.();
  popover.showPopover();
}
