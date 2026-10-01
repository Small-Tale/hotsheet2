export interface PaintScheduler {
  requestAnimationFrame(callback: FrameRequestCallback): number;
  setTimeout(callback: () => void, delay?: number): number;
}

/** Resume in a task after the next animation frame so signal updates can paint first. */
export function afterBrowserPaint(scheduler: PaintScheduler = window): Promise<void> {
  return new Promise((resolve) => scheduler.requestAnimationFrame(() => scheduler.setTimeout(resolve, 0)));
}

/**
 * Whether a finished project open should leave the selection alone because the user explicitly
 * picked a project tab while the open was in flight (HS2-YVBGW3). The opened tab is already visible
 * during the open; a later explicit selection (even re-selecting the current tab) wins over the
 * open's automatic activation unless it selected the opened project itself.
 */
export function openedProjectYieldsToSelection(
  selectionAtStart: number | undefined,
  selectionNow: number | undefined,
  currentProjectId: string | undefined,
  openedProjectId: string,
): boolean {
  return (
    selectionAtStart !== undefined &&
    selectionNow !== undefined &&
    selectionNow !== selectionAtStart &&
    currentProjectId !== openedProjectId
  );
}
