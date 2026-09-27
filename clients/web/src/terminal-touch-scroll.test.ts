import { describe, expect, it } from 'vitest';

import { createTerminalLineScroller, createTouchScrollController } from './terminal-touch-scroll';

function harness() {
  const wheels: number[] = [];
  let pending: ((time: number) => void) | undefined,
    cancelled = 0;
  const controller = createTouchScrollController({
    wheel: (delta) => wheels.push(delta),
    frame: (callback) => {
      pending = callback;
      return () => {
        cancelled += 1;
        pending = undefined;
      };
    },
  });
  const runFrames = (from: number, count: number) => {
    let time = from;
    for (let index = 0; index < count && pending; index += 1) {
      const callback = pending;
      pending = undefined;
      time += 16;
      callback(time);
    }
    return time;
  };
  return {
    controller,
    wheels,
    runFrames,
    hasPending: () => Boolean(pending),
    cancelled: () => cancelled,
  };
}

describe('terminal touch scrolling (HS2-KFBRSB)', () => {
  it('leaves a tap (movement under the threshold) alone so it can still focus the terminal', () => {
    const { controller, wheels, hasPending } = harness();
    controller.start({ y: 200, time: 0 });
    expect(controller.move({ y: 205, time: 16 })).toBe(false);
    expect(controller.scrolling).toBe(false);
    controller.end({ y: 205, time: 40 });
    expect(wheels).toEqual([]);
    expect(hasPending()).toBe(false);
  });

  it('turns a drag down into upward wheel deltas (older output) and a drag up into downward deltas', () => {
    const { controller, wheels } = harness();
    controller.start({ y: 100, time: 0 });
    expect(controller.move({ y: 112, time: 16 })).toBe(true);
    expect(controller.move({ y: 132, time: 32 })).toBe(true);
    expect(wheels).toEqual([-12, -20]);
    controller.cancel();
    wheels.length = 0;
    controller.start({ y: 300, time: 100 });
    controller.move({ y: 280, time: 116 });
    expect(wheels).toEqual([20]);
  });

  it('glides after a fast flick, decays to a stop, and a new touch cancels the glide', () => {
    const { controller, wheels, runFrames, hasPending, cancelled } = harness();
    controller.start({ y: 100, time: 0 });
    controller.move({ y: 140, time: 16 });
    controller.move({ y: 180, time: 32 });
    controller.end({ y: 180, time: 40 });
    expect(hasPending()).toBe(true);
    const flicked = wheels.length;
    runFrames(40, 3);
    const glide = wheels.slice(flicked);
    expect(glide.length).toBe(3);
    expect(glide.every((delta) => delta < 0)).toBe(true);
    expect(Math.abs(glide[2])).toBeLessThan(Math.abs(glide[0]));
    // A new touch stops the glide immediately.
    controller.start({ y: 400, time: 200 });
    expect(cancelled()).toBe(1);
    expect(hasPending()).toBe(false);
    // An unobstructed glide eventually stops on its own.
    controller.move({ y: 440, time: 216 });
    controller.end({ y: 440, time: 220 });
    runFrames(220, 1000);
    expect(hasPending()).toBe(false);
  });

  it('does not glide after a drag that paused before lifting, and ignores moves without a start', () => {
    const { controller, hasPending, wheels } = harness();
    expect(controller.move({ y: 50, time: 0 })).toBe(false);
    controller.start({ y: 100, time: 0 });
    controller.move({ y: 150, time: 16 });
    controller.end({ y: 150, time: 400 });
    expect(hasPending()).toBe(false);
    expect(wheels).toEqual([-50]);
    // Empty then refill: a second gesture after the first behaves the same.
    controller.start({ y: 100, time: 500 });
    controller.move({ y: 90, time: 516 });
    expect(controller.scrolling).toBe(true);
    controller.cancel();
    expect(controller.scrolling).toBe(false);
  });
});

describe('pixel-to-row terminal scrolling (HS2-KFBRSB)', () => {
  function target(alternate = false, applicationCursor = false, height = 10) {
    const scrolled: number[] = [],
      sent: string[] = [];
    const scroll = createTerminalLineScroller({
      rowHeight: () => height,
      alternateScreen: () => alternate,
      applicationCursorKeys: () => applicationCursor,
      scrollLines: (lines) => scrolled.push(lines),
      sendInput: (data) => sent.push(data),
    });
    return { scroll, scrolled, sent };
  }

  it('accumulates sub-row pixels and scrolls normal-buffer scrollback by whole rows in both directions', () => {
    const { scroll, scrolled, sent } = target();
    scroll(-4);
    scroll(-4);
    expect(scrolled).toEqual([]);
    scroll(-4);
    expect(scrolled).toEqual([-1]);
    scroll(-25);
    expect(scrolled).toEqual([-1, -2]);
    scroll(40);
    expect(scrolled).toEqual([-1, -2, 3]);
    expect(sent).toEqual([]);
  });

  it('sends arrow keys to an alternate-screen app, honoring application cursor mode', () => {
    const normal = target(true, false);
    normal.scroll(-30);
    normal.scroll(20);
    expect(normal.sent).toEqual(['\u001b[A\u001b[A\u001b[A', '\u001b[B\u001b[B']);
    expect(normal.scrolled).toEqual([]);
    const application = target(true, true);
    application.scroll(10);
    expect(application.sent).toEqual(['\u001bOB']);
  });

  it('does nothing before the terminal has a measurable row height', () => {
    const { scroll, scrolled, sent } = target(false, false, 0);
    scroll(-500);
    expect([scrolled, sent]).toEqual([[], []]);
  });
});
