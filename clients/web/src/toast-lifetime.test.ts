import { describe, expect, it, vi } from 'vitest';

import { createToastLifetime, type ToastState } from './toast-lifetime';

describe('application toast lifetime', () => {
  it('keeps a replacement for its own 2.5 seconds and ignores stale hides', () => {
    vi.useFakeTimers();
    try {
      const states: ToastState[] = [];
      const toast = createToastLifetime((state) => states.push(state));
      toast.show('First');
      vi.advanceTimersByTime(1_000);
      toast.show('Second');
      toast.hide(1); // An earlier Web Awesome item finished its hide animation.
      vi.advanceTimersByTime(1_499);
      expect(states.at(-1)).toEqual({ message: 'Second', generation: 2 });
      vi.advanceTimersByTime(1_001);
      expect(states.at(-1)).toEqual({ message: '', generation: 2 });
      toast.dispose();
    } finally {
      vi.useRealTimers();
    }
  });

  it('synchronizes manual dismissal and leaves a later toast independent', () => {
    vi.useFakeTimers();
    try {
      const states: ToastState[] = [];
      const toast = createToastLifetime((state) => states.push(state));
      toast.show('Dismiss me');
      toast.hide(1);
      expect(states.at(-1)?.message).toBe('');
      toast.show('Replacement');
      vi.advanceTimersByTime(2_500);
      expect(states.at(-1)).toEqual({ message: '', generation: 2 });
      toast.dispose();
    } finally {
      vi.useRealTimers();
    }
  });

  it('keeps a toast for a requested longer lifetime and the next one for the default (GH-5)', () => {
    vi.useFakeTimers();
    try {
      const states: ToastState[] = [];
      const toast = createToastLifetime((state) => states.push(state));
      toast.show('Long explanation', 12_000);
      vi.advanceTimersByTime(11_999);
      expect(states.at(-1)).toEqual({ message: 'Long explanation', generation: 1 });
      vi.advanceTimersByTime(1);
      expect(states.at(-1)).toEqual({ message: '', generation: 1 });
      toast.show('Short');
      vi.advanceTimersByTime(2_500);
      expect(states.at(-1)).toEqual({ message: '', generation: 2 });
      toast.dispose();
    } finally {
      vi.useRealTimers();
    }
  });
});
