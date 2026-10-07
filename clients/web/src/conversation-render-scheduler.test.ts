import { afterEach, describe, expect, it, vi } from 'vitest';

import { createConversationRenderScheduler } from './conversation-render-scheduler';

afterEach(() => vi.useRealTimers());

describe('conversation output render cadence (HS2-0PFQ8V)', () => {
  it('bounds sustained events to one render per interval', () => {
    vi.useFakeTimers();
    const render = vi.fn(),
      scheduler = createConversationRenderScheduler(render, 80);
    for (let index = 0; index < 20; index += 1) {
      scheduler.schedule();
      vi.advanceTimersByTime(10);
    }
    expect(render).toHaveBeenCalledTimes(2);
    vi.advanceTimersByTime(80);
    expect(render).toHaveBeenCalledTimes(3);
    scheduler.schedule();
    scheduler.immediate();
    vi.advanceTimersByTime(80);
    expect(render).toHaveBeenCalledTimes(4);
  });

  it('flushes a pending update once and leaves idle renders alone', () => {
    vi.useFakeTimers();
    const render = vi.fn(),
      scheduler = createConversationRenderScheduler(render);
    scheduler.flush();
    expect(render).not.toHaveBeenCalled();
    scheduler.schedule();
    scheduler.schedule();
    scheduler.flush();
    scheduler.flush();
    vi.advanceTimersByTime(100);
    expect(render).toHaveBeenCalledTimes(1);
    scheduler.immediate();
    expect(render).toHaveBeenCalledTimes(2);
  });
});
