/**
 * Touch scrolling for interactive terminals (HS2-KFBRSB).
 *
 * xterm scrolls from wheel events only (and ignores script-dispatched ones), so a finger drag on a
 * phone did nothing. A vertical single-finger drag is translated into pixel deltas, followed by a
 * short momentum glide, and `createTerminalLineScroller` turns those pixels into whole rows the way
 * xterm's wheel does: scrollback in the normal buffer, arrow keys for alternate-screen apps. A tap
 * (movement below the threshold) is left alone so it still focuses the terminal and opens the
 * keyboard.
 */
export interface TouchScrollPoint {
  y: number;
  time: number;
}

export interface TouchScrollOptions {
  /** Deliver a pixel wheel delta (positive scrolls toward newer output, like `WheelEvent.deltaY`). */
  wheel: (deltaY: number) => void;
  /** Schedule one momentum frame; returns a cancel function. Local UI timing only, never network. */
  frame: (callback: (time: number) => void) => () => void;
  /** Movement before a touch counts as a scroll rather than a tap. */
  threshold?: number;
  /** Per-frame velocity retention for the momentum glide (0–1). */
  friction?: number;
}

export interface TouchScrollController {
  start(point: TouchScrollPoint): void;
  /** Returns whether the move scrolled (the caller then prevents the native gesture). */
  move(point: TouchScrollPoint): boolean;
  end(point?: TouchScrollPoint): void;
  cancel(): void;
  readonly scrolling: boolean;
}

const MIN_MOMENTUM = 0.02; // px per ms

export function createTouchScrollController({
  wheel,
  frame,
  threshold = 8,
  friction = 0.95,
}: TouchScrollOptions): TouchScrollController {
  let origin: TouchScrollPoint | undefined,
    last: TouchScrollPoint | undefined,
    velocity = 0,
    scrolling = false,
    cancelMomentum: (() => void) | undefined;
  const stopMomentum = () => {
    cancelMomentum?.();
    cancelMomentum = undefined;
  };
  const glide = (previous: number) => {
    cancelMomentum = frame((time) => {
      const elapsed = Math.max(1, time - previous);
      velocity *= friction ** (elapsed / 16);
      if (Math.abs(velocity) < MIN_MOMENTUM) {
        cancelMomentum = undefined;
        return;
      }
      wheel(-velocity * elapsed);
      glide(time);
    });
  };
  return {
    start(point) {
      stopMomentum();
      origin = point;
      last = point;
      velocity = 0;
      scrolling = false;
    },
    move(point) {
      if (!origin || !last) return false;
      if (!scrolling && Math.abs(point.y - origin.y) < threshold) return false;
      scrolling = true;
      const delta = point.y - last.y,
        elapsed = Math.max(1, point.time - last.time);
      // Dragging down reveals older output, which is a negative (upward) wheel delta.
      if (delta) wheel(-delta);
      velocity = 0.8 * (delta / elapsed) + 0.2 * velocity;
      last = point;
      return true;
    },
    end(point) {
      if (scrolling && point && last && point.time - last.time > 100) velocity = 0;
      if (scrolling && Math.abs(velocity) >= MIN_MOMENTUM) glide(point?.time ?? last?.time ?? 0);
      origin = undefined;
      last = undefined;
      scrolling = false;
    },
    cancel() {
      stopMomentum();
      origin = undefined;
      last = undefined;
      velocity = 0;
      scrolling = false;
    },
    get scrolling() {
      return scrolling;
    },
  };
}

export interface LineScrollTarget {
  /** Rendered height of one row in CSS pixels (including any fitted scale). */
  rowHeight(): number;
  alternateScreen(): boolean;
  applicationCursorKeys(): boolean;
  scrollLines(lines: number): void;
  sendInput(data: string): void;
}

/**
 * Accumulate pixel deltas into whole rows. Normal-buffer scrollback moves the viewport; an
 * alternate-screen app (nano, less, vim) receives arrow keys instead, as xterm's wheel sends there.
 */
export function createTerminalLineScroller(target: LineScrollTarget): (deltaY: number) => void {
  let pending = 0;
  return (deltaY) => {
    const height = target.rowHeight();
    if (!(height > 0)) return;
    pending += deltaY;
    const lines = Math.trunc(pending / height);
    if (!lines) return;
    pending -= lines * height;
    if (!target.alternateScreen()) {
      target.scrollLines(lines);
      return;
    }
    const prefix = target.applicationCursorKeys() ? '\u001bO' : '\u001b[';
    target.sendInput(`${prefix}${lines < 0 ? 'A' : 'B'}`.repeat(Math.abs(lines)));
  };
}
