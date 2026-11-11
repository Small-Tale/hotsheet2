import { describe, expect, it } from 'vitest';

import {
  ANNOUNCER_DOCK_BOUNDS,
  loadAnnouncerDockSize,
  normalizeAnnouncerDockSize,
  saveAnnouncerDockSize,
} from './announcer-layout';

const memory = () => {
  const values = new Map<string, string>();
  return {
    values,
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => void values.set(key, value),
  };
};

describe('docked announcer height (HS2-BS0XFK)', () => {
  it('falls back to the default without a usable stored height', () => {
    const storage = memory();
    expect(loadAnnouncerDockSize(storage)).toBe(ANNOUNCER_DOCK_BOUNDS.fallback);
    for (const value of ['', 'tall', '0', '-40', 'NaN']) {
      storage.values.set('hotsheet.layout.announcer-dock.size', value);
      expect(loadAnnouncerDockSize(storage)).toBe(ANNOUNCER_DOCK_BOUNDS.fallback);
    }
    expect(
      loadAnnouncerDockSize({
        getItem: () => {
          throw new Error('blocked');
        },
      }),
    ).toBe(ANNOUNCER_DOCK_BOUNDS.fallback);
  });

  it('round-trips a committed height within its bounds, repeatedly', () => {
    const storage = memory();
    expect(saveAnnouncerDockSize(storage, 300.4)).toBe(300);
    expect(loadAnnouncerDockSize(storage)).toBe(300);
    expect(saveAnnouncerDockSize(storage, 20)).toBe(ANNOUNCER_DOCK_BOUNDS.min);
    expect(loadAnnouncerDockSize(storage)).toBe(ANNOUNCER_DOCK_BOUNDS.min);
    expect(saveAnnouncerDockSize(storage, 9000)).toBe(ANNOUNCER_DOCK_BOUNDS.max);
    expect(loadAnnouncerDockSize(storage)).toBe(ANNOUNCER_DOCK_BOUNDS.max);
    storage.values.set('hotsheet.layout.announcer-dock.size', '9000');
    expect(loadAnnouncerDockSize(storage)).toBe(ANNOUNCER_DOCK_BOUNDS.max);
    expect(normalizeAnnouncerDockSize(ANNOUNCER_DOCK_BOUNDS.min - 1)).toBe(ANNOUNCER_DOCK_BOUNDS.min);
  });

  it('still resizes for the session when the store refuses the write', () => {
    expect(
      saveAnnouncerDockSize(
        {
          setItem: () => {
            throw new Error('quota');
          },
        },
        260,
      ),
    ).toBe(260);
  });
});
