import { describe, expect, it, vi } from 'vitest';

import { afterBrowserPaint, openedProjectYieldsToSelection, type PaintScheduler } from './project-activation';

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

describe('opened project activation versus an explicit tab choice (HS2-YVBGW3)', () => {
  it('activates the opened project when nobody selected a tab during the open', () => {
    expect(openedProjectYieldsToSelection(3, 3, 'demo', 'other')).toBe(false);
  });

  it('yields when the user selected any other tab, including re-selecting the current one', () => {
    expect(openedProjectYieldsToSelection(3, 4, 'demo', 'other')).toBe(true);
    expect(openedProjectYieldsToSelection(3, 6, 'third', 'other')).toBe(true);
  });

  it('still finishes activation when the user already selected the opened project itself', () => {
    expect(openedProjectYieldsToSelection(3, 4, 'other', 'other')).toBe(false);
  });

  it('keeps the legacy behavior when no selection counter is wired', () => {
    expect(openedProjectYieldsToSelection(undefined, undefined, 'demo', 'other')).toBe(false);
    expect(openedProjectYieldsToSelection(undefined, 2, 'demo', 'other')).toBe(false);
  });
});
