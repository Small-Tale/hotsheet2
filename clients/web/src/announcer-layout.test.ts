import { describe, expect, it } from 'vitest';

import {
  ANNOUNCER_DOCK_BOUNDS,
  loadAnnouncerDetached,
  loadAnnouncerDockSize,
  loadAnnouncerFloatingPlacement,
  normalizeAnnouncerDockSize,
  normalizeAnnouncerFloatingPlacement,
  saveAnnouncerDetached,
  saveAnnouncerDockSize,
  saveAnnouncerFloatingPlacement,
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

describe('detached announcer state (HS2-51X25Z)', () => {
  it('starts attached and walks detach, reattach, detach again', () => {
    const store = memory();
    expect(loadAnnouncerDetached(store)).toBe(false);
    for (const detached of [true, false, true, true, false]) {
      saveAnnouncerDetached(store, detached);
      expect(loadAnnouncerDetached(store)).toBe(detached);
    }
    store.values.set('hotsheet.layout.announcer-detached', 'yes');
    expect(loadAnnouncerDetached(store)).toBe(false);
  });

  it('treats a throwing store as attached and keeps working', () => {
    const broken = {
      getItem: () => {
        throw new Error('blocked');
      },
      setItem: () => {
        throw new Error('blocked');
      },
    };
    expect(loadAnnouncerDetached(broken)).toBe(false);
    expect(() => {
      saveAnnouncerDetached(broken, true);
    }).not.toThrow();
    expect(loadAnnouncerFloatingPlacement(broken)).toBeUndefined();
    expect(saveAnnouncerFloatingPlacement(broken, { x: 1, y: 2 })).toEqual({ x: 1, y: 2 });
  });

  it('keeps only usable placements, rounding them', () => {
    expect(normalizeAnnouncerFloatingPlacement(null)).toBeUndefined();
    expect(normalizeAnnouncerFloatingPlacement({ x: 'a', y: 1 })).toBeUndefined();
    expect(normalizeAnnouncerFloatingPlacement({ x: Number.NaN, y: 1 })).toBeUndefined();
    expect(normalizeAnnouncerFloatingPlacement({ x: 10.4, y: 20.6 })).toEqual({ x: 10, y: 21 });
    expect(normalizeAnnouncerFloatingPlacement({ x: 1, y: 2, width: 0, height: -4 })).toEqual({ x: 1, y: 2 });
    expect(normalizeAnnouncerFloatingPlacement({ x: 1, y: 2, width: 320.2, height: 240.7 })).toEqual({
      x: 1,
      y: 2,
      width: 320,
      height: 241,
    });
  });

  it('round-trips moves and resizes, ignoring an unusable report and a corrupt store', () => {
    const store = memory();
    expect(loadAnnouncerFloatingPlacement(store)).toBeUndefined();
    saveAnnouncerFloatingPlacement(store, { x: 40, y: 60 });
    expect(loadAnnouncerFloatingPlacement(store)).toEqual({ x: 40, y: 60 });
    saveAnnouncerFloatingPlacement(store, { x: 48, y: 60, width: 400, height: 300 });
    expect(loadAnnouncerFloatingPlacement(store)).toEqual({ x: 48, y: 60, width: 400, height: 300 });
    expect(saveAnnouncerFloatingPlacement(store, null)).toBeUndefined();
    expect(loadAnnouncerFloatingPlacement(store)).toEqual({ x: 48, y: 60, width: 400, height: 300 });
    store.values.set('hotsheet.layout.announcer-floating.placement', '{not json');
    expect(loadAnnouncerFloatingPlacement(store)).toBeUndefined();
  });
});
