import { afterEach, expect, it, vi } from 'vitest';

import { focusTopTicketReader } from './ticket-reader-focus';

afterEach(() => vi.unstubAllGlobals());

function fixture() {
  const frames: FrameRequestCallback[] = [];
  vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => frames.push(callback));
  const input = {},
    outside = {},
    close = { focus: vi.fn() };
  const reader = {
    contains: (node: unknown) => node === input || node === close,
    querySelector: () => close,
  };
  let active: unknown = outside,
    current: typeof reader | null = reader;
  const root = {
    get activeElement() {
      return active;
    },
    querySelector: () => current,
  } as unknown as Document;
  return {
    root,
    reader,
    input,
    outside,
    close,
    active: (node: unknown) => {
      active = node;
    },
    current: (node: typeof reader | null) => {
      current = node;
    },
    frame: () => {
      for (const callback of frames.splice(0)) callback(0);
    },
  };
}

it('HS2-XBWGP6 preserves input focused while the initial reader focus is pending', () => {
  const f = fixture();
  focusTopTicketReader(f.root);
  f.frame();
  f.active(f.input);
  f.frame();
  expect(f.close.focus).not.toHaveBeenCalled();
});

it('HS2-XBWGP6 contains outside focus and leaves an already focused reader control alone', () => {
  const f = fixture();
  focusTopTicketReader(f.root);
  f.frame();
  f.frame();
  expect(f.close.focus).toHaveBeenCalledExactlyOnceWith({ preventScroll: true });
  f.active(f.close);
  focusTopTicketReader(f.root);
  f.frame();
  f.frame();
  expect(f.close.focus).toHaveBeenCalledOnce();
});

it('HS2-XBWGP6 resolves the current reader when nesting changes or the reader closes', () => {
  const f = fixture(),
    nextClose = { focus: vi.fn() };
  focusTopTicketReader(f.root);
  f.frame();
  f.current({ contains: () => false, querySelector: () => nextClose });
  f.frame();
  expect(nextClose.focus).toHaveBeenCalledOnce();
  expect(f.close.focus).not.toHaveBeenCalled();
  focusTopTicketReader(f.root);
  f.frame();
  f.current(null);
  f.frame();
  expect(nextClose.focus).toHaveBeenCalledOnce();
  f.current(f.reader);
  focusTopTicketReader(f.root);
  f.frame();
  f.frame();
  expect(f.close.focus).toHaveBeenCalledOnce();
});
