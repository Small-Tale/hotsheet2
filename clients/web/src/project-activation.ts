export interface PaintScheduler {
  requestAnimationFrame(callback: FrameRequestCallback): number;
  setTimeout(callback: () => void, delay?: number): number;
}

/** Resume in a task after the next animation frame so signal updates can paint first. */
export function afterBrowserPaint(scheduler: PaintScheduler = window): Promise<void> {
  return new Promise(resolve => scheduler.requestAnimationFrame(() => scheduler.setTimeout(resolve, 0)));
}
