import { describe, expect, it } from 'vitest';

import {
  chordFromEvent,
  chordsEqual,
  findChordConflict,
  formatChord,
  KEYBOARD_SHORTCUT_STORAGE_KEY,
  KEYBOARD_SHORTCUTS,
  loadShortcutOverrides,
  matchesChord,
  matchesShortcut,
  resolveChord,
  saveShortcutOverrides,
  type ShortcutChord,
} from './keyboard-shortcuts';

function fakeStorage(initial: Record<string, string> = {}) {
  const map = new Map(Object.entries(initial));
  return {
    map,
    getItem: (key: string) => map.get(key) ?? null,
    setItem: (key: string, value: string) => {
      map.set(key, value);
    },
    removeItem: (key: string) => {
      map.delete(key);
    },
  };
}

function key(over: Partial<KeyboardEvent>): Pick<KeyboardEvent, 'key' | 'metaKey' | 'ctrlKey' | 'shiftKey' | 'altKey'> {
  return { key: 'a', metaKey: false, ctrlKey: false, shiftKey: false, altKey: false, ...over };
}

describe('keyboard-shortcuts registry', () => {
  it('has a stable, non-empty, unique-id catalog with valid defaults', () => {
    expect(KEYBOARD_SHORTCUTS.length).toBeGreaterThan(10);
    const ids = KEYBOARD_SHORTCUTS.map((s) => s.id);
    expect(new Set(ids).size).toBe(ids.length);
    for (const shortcut of KEYBOARD_SHORTCUTS) {
      expect(shortcut.label).toBeTruthy();
      expect(shortcut.defaultChord.key.length).toBeGreaterThan(0);
    }
    // The rebindable global chords exist.
    for (const id of ['open-search', 'undo', 'redo'])
      expect(KEYBOARD_SHORTCUTS.find((s) => s.id === id)?.editable).toBe(true);
  });

  it('defines the view/panel and tab-cycling command shortcuts as rebindable (HS2-9SHYWD)', () => {
    const expected: Record<string, ShortcutChord> = {
      'toggle-left-sidebar': { key: 'b', mod: true },
      'toggle-right-sidebar': { key: 'b', mod: true, alt: true, shift: true },
      'toggle-bottom-drawer': { key: 'j', mod: true },
      'view-list': { key: 'l', mod: true, shift: true },
      'view-board': { key: 'b', mod: true, shift: true },
      'view-notifications': { key: 'm', mod: true, shift: true },
      'view-settings': { key: 's', mod: true, alt: true },
      'view-workspace-grid': { key: 'g', mod: true, shift: true },
      'view-all-stats': { key: 'd', mod: true, shift: true },
      'new-ticket': { key: 'c' },
      'project-tab-previous': { key: 'ArrowLeft', mod: true, alt: true, shift: true },
      'project-tab-next': { key: 'ArrowRight', mod: true, alt: true, shift: true },
      'drawer-tab-previous': { key: 'ArrowUp', mod: true, alt: true, shift: true },
      'drawer-tab-next': { key: 'ArrowDown', mod: true, alt: true, shift: true },
    };
    for (const [id, chord] of Object.entries(expected)) {
      const def = KEYBOARD_SHORTCUTS.find((s) => s.id === id);
      expect(def, id).toBeTruthy();
      expect(def?.editable, id).toBe(true);
      expect(def?.defaultChord, id).toEqual(chord);
    }
  });

  it('does not assign the Safari-reserved chords reported by HS2-Q1BH0V', () => {
    const safariReserved: ShortcutChord[] = [
      { key: ',', mod: true },
      { key: 'b', mod: true, alt: true },
      { key: 'ArrowLeft', mod: true, alt: true },
      { key: 'ArrowRight', mod: true, alt: true },
      { key: 'ArrowUp', mod: true, alt: true },
      { key: 'ArrowDown', mod: true, alt: true },
    ];
    const collisions = KEYBOARD_SHORTCUTS.filter((shortcut) => shortcut.editable)
      .filter((shortcut) => safariReserved.some((chord) => chordsEqual(shortcut.defaultChord, chord)))
      .map((shortcut) => shortcut.id);
    expect(collisions).toEqual([]);
  });

  it('has no two editable shortcuts sharing a default chord (no self-conflicts)', () => {
    const editable = KEYBOARD_SHORTCUTS.filter((s) => s.editable);
    for (const shortcut of editable) {
      const clash = editable.find(
        (other) => other.id !== shortcut.id && chordsEqual(other.defaultChord, shortcut.defaultChord),
      );
      expect(clash, `${shortcut.id} vs ${clash?.id}`).toBeUndefined();
    }
  });

  it('matches events against the effective chord with platform-correct modifiers', () => {
    const overrides: Record<string, ShortcutChord> = {};
    // Apple: mod is metaKey.
    expect(matchesShortcut('open-search', key({ key: 'k', metaKey: true }), overrides, true)).toBe(true);
    expect(matchesShortcut('open-search', key({ key: 'k', ctrlKey: true }), overrides, true)).toBe(false);
    // Non-Apple: mod is ctrlKey.
    expect(matchesShortcut('open-search', key({ key: 'k', ctrlKey: true }), overrides, false)).toBe(true);
    // redo requires shift; undo forbids it.
    expect(matchesShortcut('redo', key({ key: 'z', metaKey: true, shiftKey: true }), overrides, true)).toBe(true);
    expect(matchesShortcut('undo', key({ key: 'z', metaKey: true, shiftKey: true }), overrides, true)).toBe(false);
    expect(matchesShortcut('undo', key({ key: 'z', metaKey: true }), overrides, true)).toBe(true);
    // The wrong modifier (the non-primary one) must not match.
    expect(matchesChord(key({ key: 'k', metaKey: true, ctrlKey: true }), { key: 'k', mod: true }, true)).toBe(false);
  });

  it('captures a chord from an event, ignoring lone modifier presses', () => {
    expect(chordFromEvent(key({ key: 'Shift', shiftKey: true }), true)).toBeUndefined();
    expect(chordFromEvent(key({ key: 'J', metaKey: true, shiftKey: true }), true)).toEqual({
      key: 'j',
      mod: true,
      shift: true,
      alt: false,
    });
    expect(chordFromEvent(key({ key: 'ArrowUp' }), true)).toEqual({
      key: 'ArrowUp',
      mod: false,
      shift: false,
      alt: false,
    });
  });

  it('formats chords for Apple and non-Apple platforms', () => {
    expect(formatChord({ key: 'k', mod: true }, true)).toBe('⌘K');
    expect(formatChord({ key: 'k', mod: true }, false)).toBe('Ctrl+K');
    expect(formatChord({ key: 'z', mod: true, shift: true }, false)).toBe('Ctrl+Shift+Z');
    expect(formatChord({ key: 'ArrowLeft', alt: true, shift: true }, true)).toBe('⌥⇧←');
    expect(formatChord({ key: ' ' }, false)).toBe('Space');
    expect(formatChord(undefined)).toBe('Unassigned');
  });

  it('detects a conflict only among other editable shortcuts', () => {
    // Rebind undo onto open-search's default chord → conflict reported for each other's chord.
    const overrides: Record<string, ShortcutChord> = { undo: { key: 'k', mod: true } };
    expect(findChordConflict('undo', { key: 'k', mod: true }, overrides)?.id).toBe('open-search');
    expect(findChordConflict('open-search', { key: 'k', mod: true }, overrides)?.id).toBe('undo');
    // A unique chord conflicts with nothing.
    expect(findChordConflict('undo', { key: 'j', mod: true, alt: true }, overrides)).toBeUndefined();
  });
});

describe('override persistence (transition + adversarial)', () => {
  it('round-trips an editable override and prefers it over the default', () => {
    const storage = fakeStorage();
    const custom: ShortcutChord = { key: 'j', mod: true };
    saveShortcutOverrides({ 'open-search': custom }, storage);
    const loaded = loadShortcutOverrides(storage);
    expect(loaded['open-search']).toEqual({ key: 'j', mod: true, shift: false, alt: false });
    expect(resolveChord('open-search', loaded)).toEqual({ key: 'j', mod: true, shift: false, alt: false });
    // The override now matches, and the old default no longer does.
    expect(matchesShortcut('open-search', key({ key: 'j', metaKey: true }), loaded, true)).toBe(true);
    expect(matchesShortcut('open-search', key({ key: 'k', metaKey: true }), loaded, true)).toBe(false);
    // Resetting to empty restores the default binding.
    saveShortcutOverrides({}, storage);
    expect(storage.getItem(KEYBOARD_SHORTCUT_STORAGE_KEY)).toBeNull();
    const reset = loadShortcutOverrides(storage);
    expect(resolveChord('open-search', reset)).toEqual({ key: 'k', mod: true });
    expect(matchesShortcut('open-search', key({ key: 'k', metaKey: true }), reset, true)).toBe(true);
  });

  it('drops overrides for unknown or non-editable ids, and tolerates corrupt storage', () => {
    // A non-editable id (move-selection-up, a fixed ARIA affordance) and an unknown id must not be honored.
    const storage = fakeStorage({
      [KEYBOARD_SHORTCUT_STORAGE_KEY]: JSON.stringify({
        'move-selection-up': { key: 'q', mod: true },
        'not-a-shortcut': { key: 'w' },
        'open-search': { key: 'p', mod: true },
      }),
    });
    const loaded = loadShortcutOverrides(storage);
    expect(loaded).toEqual({ 'open-search': { key: 'p', mod: true, shift: false, alt: false } });
    // move-selection-up keeps its fixed default despite the stored override.
    expect(resolveChord('move-selection-up', loaded)).toEqual({ key: 'ArrowUp' });
    // Corrupt JSON and missing storage both yield no overrides.
    expect(loadShortcutOverrides(fakeStorage({ [KEYBOARD_SHORTCUT_STORAGE_KEY]: '{not json' }))).toEqual({});
    expect(loadShortcutOverrides(fakeStorage())).toEqual({});
  });

  it('survives an out-of-order rebind → conflict → reset-one → reset-all sequence', () => {
    const storage = fakeStorage();
    // Rebind redo, then rebind undo onto redo's new chord (a conflict), then reset undo, then reset all.
    saveShortcutOverrides({ redo: { key: 'y', mod: true } }, storage);
    let state = loadShortcutOverrides(storage);
    expect(chordsEqual(resolveChord('redo', state), { key: 'y', mod: true })).toBe(true);
    state = { ...state, undo: { key: 'y', mod: true } };
    saveShortcutOverrides(state, storage);
    state = loadShortcutOverrides(storage);
    expect(findChordConflict('undo', resolveChord('undo', state)!, state)?.id).toBe('redo');
    // Reset just undo: redo override persists, undo returns to default.
    state = Object.fromEntries(Object.entries(state).filter(([id]) => id !== 'undo'));
    saveShortcutOverrides(state, storage);
    state = loadShortcutOverrides(storage);
    expect(resolveChord('undo', state)).toEqual({ key: 'z', mod: true });
    expect(chordsEqual(resolveChord('redo', state), { key: 'y', mod: true })).toBe(true);
    // Reset all: everything returns to defaults.
    saveShortcutOverrides({}, storage);
    state = loadShortcutOverrides(storage);
    expect(state).toEqual({});
    expect(resolveChord('redo', state)).toEqual({ key: 'z', mod: true, shift: true });
  });
});
