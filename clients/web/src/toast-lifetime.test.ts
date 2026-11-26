import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import {
  createToastLifetime,
  TOAST_MAX_MS,
  TOAST_MIN_MS,
  TOAST_MS_PER_WORD,
  TOAST_PERSISTENT,
  toastReadingTime,
  type ToastState,
} from './toast-lifetime';

const words = (count: number) => Array.from({ length: count }, (_, index) => `word${index}`).join(' ');

describe('HS2-PC7VRA toast reading time', () => {
  it('pins the rule: 300 ms a word, a 2.5-second floor and a 20-second cap', () => {
    expect([TOAST_MIN_MS, TOAST_MS_PER_WORD, TOAST_MAX_MS]).toEqual([2_500, 300, 20_000]);
  });

  it.each([
    ['an empty message', '', 2_500],
    ['only whitespace', ' \n\t ', 2_500],
    ['one word', 'Copied.', 2_500],
    ['a short acknowledgement', 'Attachment removed.', 2_500],
    ['eight words, still under the floor', words(8), 2_500],
    ['nine words, the first above the floor', words(9), 2_700],
    ['a long error', words(17), 5_100],
    ['66 words, just under the cap', words(66), 19_800],
    ['67 words, at the cap', words(67), 20_000],
    ['far past the cap', words(500), 20_000],
  ])('%s', (_label, message, expected) => {
    expect(toastReadingTime(message)).toBe(expected);
  });

  it('counts words across any run of whitespace, not characters', () => {
    expect(toastReadingTime(`  ${words(10).replaceAll(' ', ' \n\t  ')}  `)).toBe(3_000);
    expect(toastReadingTime('x'.repeat(2_000))).toBe(2_500);
  });
});

describe('application toast lifetime', () => {
  let states: ToastState[];
  const last = () => states.at(-1);
  beforeEach(() => {
    vi.useFakeTimers();
    states = [];
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it('keeps a replacement for its own 2.5 seconds and ignores stale hides', () => {
    const toast = createToastLifetime((state) => states.push(state));
    toast.show('First');
    vi.advanceTimersByTime(1_000);
    toast.show('Second');
    toast.hide(1); // An earlier Web Awesome item finished its hide animation.
    vi.advanceTimersByTime(1_499);
    expect(last()).toEqual({ message: 'Second', generation: 2 });
    vi.advanceTimersByTime(1_001);
    expect(last()).toEqual({ message: '', generation: 2 });
    toast.dispose();
  });

  it('synchronizes manual dismissal and leaves a later toast independent', () => {
    const toast = createToastLifetime((state) => states.push(state));
    toast.show('Dismiss me');
    toast.hide(1);
    expect(last()?.message).toBe('');
    toast.show('Replacement');
    vi.advanceTimersByTime(2_500);
    expect(last()).toEqual({ message: '', generation: 2 });
    toast.dispose();
  });

  it('keeps a long message for its reading time without the caller asking (HS2-PC7VRA)', () => {
    const toast = createToastLifetime((state) => states.push(state));
    const message = words(30); // 9 seconds
    toast.show(message);
    vi.advanceTimersByTime(8_999);
    expect(last()).toEqual({ message, generation: 1 });
    vi.advanceTimersByTime(1);
    expect(last()).toEqual({ message: '', generation: 1 });
    // The app's `showToast` forwards an absent lifetime as `undefined`: still the reading time.
    toast.show(message, undefined);
    vi.advanceTimersByTime(8_999);
    expect(last()).toEqual({ message, generation: 2 });
    vi.advanceTimersByTime(1);
    expect(last()).toEqual({ message: '', generation: 2 });
    toast.dispose();
  });

  it('caps a very long message at 20 seconds (HS2-PC7VRA)', () => {
    const toast = createToastLifetime((state) => states.push(state));
    toast.show(words(400));
    vi.advanceTimersByTime(19_999);
    expect(last()?.message).not.toBe('');
    vi.advanceTimersByTime(1);
    expect(last()?.message).toBe('');
    toast.dispose();
  });

  it('gives a short replacement of a long toast only its own 2.5 seconds (HS2-PC7VRA)', () => {
    const toast = createToastLifetime((state) => states.push(state));
    toast.show(words(60));
    vi.advanceTimersByTime(1_000);
    toast.show('Short');
    vi.advanceTimersByTime(2_499);
    expect(last()).toEqual({ message: 'Short', generation: 2 });
    vi.advanceTimersByTime(1);
    expect(last()).toEqual({ message: '', generation: 2 });
    // The long toast's timer was cleared: nothing publishes later.
    const published = states.length;
    vi.advanceTimersByTime(30_000);
    expect(states).toHaveLength(published);
    toast.dispose();
  });

  it('honors an explicit lifetime longer or shorter than the reading time (GH-5, HS2-PC7VRA)', () => {
    const toast = createToastLifetime((state) => states.push(state));
    toast.show('Long explanation', 12_000);
    vi.advanceTimersByTime(11_999);
    expect(last()).toEqual({ message: 'Long explanation', generation: 1 });
    vi.advanceTimersByTime(1);
    expect(last()).toEqual({ message: '', generation: 1 });
    const long = words(40); // 12 seconds of reading time
    toast.show(long, 1_000);
    vi.advanceTimersByTime(1_000);
    expect(last()).toEqual({ message: '', generation: 2 });
    toast.show('Short');
    vi.advanceTimersByTime(2_500);
    expect(last()).toEqual({ message: '', generation: 3 });
    toast.dispose();
  });

  it.each([
    ['TOAST_PERSISTENT', TOAST_PERSISTENT],
    ['zero', 0],
    ['a negative lifetime', -1],
    ['NaN', Number.NaN],
  ])('keeps a toast with %s until it is dismissed or replaced (HS2-PC7VRA)', (_label, lifetime) => {
    const toast = createToastLifetime((state) => states.push(state));
    toast.show('Stays', lifetime);
    vi.advanceTimersByTime(TOAST_MAX_MS * 10);
    expect(last()).toEqual({ message: 'Stays', generation: 1 });
    expect(vi.getTimerCount()).toBe(0);
    toast.hide(1);
    expect(last()).toEqual({ message: '', generation: 1 });
    toast.show('Stays again', lifetime);
    toast.show('Timed');
    vi.advanceTimersByTime(2_500);
    expect(last()).toEqual({ message: '', generation: 3 });
    toast.dispose();
  });

  it('a timed toast replaced by a persistent one never hides it (HS2-PC7VRA)', () => {
    const toast = createToastLifetime((state) => states.push(state));
    toast.show('Timed');
    toast.show('Persistent', TOAST_PERSISTENT);
    vi.advanceTimersByTime(60_000);
    expect(last()).toEqual({ message: 'Persistent', generation: 2 });
    toast.dispose();
  });

  it('dispose cancels a pending hide', () => {
    const toast = createToastLifetime((state) => states.push(state));
    toast.show(words(20));
    toast.dispose();
    vi.advanceTimersByTime(TOAST_MAX_MS);
    expect(last()?.message).not.toBe('');
    expect(vi.getTimerCount()).toBe(0);
  });
});
