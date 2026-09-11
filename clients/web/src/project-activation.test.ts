import { describe, expect, it, vi } from 'vitest';

import { afterBrowserPaint, type PaintScheduler } from './project-activation';

describe('project activation scheduling', () => {
  it('defers refresh work to a task after the next animation frame', async () => {
    let frame: FrameRequestCallback | undefined;
    let task: (() => void) | undefined;
    const requestAnimationFrame = vi.fn((callback: FrameRequestCallback) => {
      frame = callback;
      return 1;
    });
    const setTimeout = vi.fn((callback: () => void) => {
      task = callback;
      return 2;
    });
    const scheduler: PaintScheduler = {
      requestAnimationFrame,
      setTimeout,
    };
    let resumed = false;
    const pending = afterBrowserPaint(scheduler).then(() => {
      resumed = true;
    });

    expect(requestAnimationFrame).toHaveBeenCalledOnce();
    expect(setTimeout).not.toHaveBeenCalled();
    frame?.(16);
    expect(setTimeout).toHaveBeenCalledWith(expect.any(Function), 0);
    expect(resumed).toBe(false);
    task?.();
    await pending;
    expect(resumed).toBe(true);
  });
});
