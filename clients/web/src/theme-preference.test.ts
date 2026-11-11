import { describe, expect, it } from 'vitest';

import {
  applyThemePreference,
  parseThemePreference,
  readThemePreference,
  storeThemePreference,
  THEME_STORAGE_KEY,
  type ThemePreference,
} from './theme-preference';

function memoryStorage(initial: Record<string, string> = {}) {
  const values = new Map(Object.entries(initial));
  return {
    values,
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => void values.set(key, value),
    removeItem: (key: string) => void values.delete(key),
  };
}

describe('theme preference (HS2-EFTHS6)', () => {
  it('reads light and dark and treats anything else as auto', () => {
    expect(parseThemePreference('light')).toBe('light');
    expect(parseThemePreference('dark')).toBe('dark');
    for (const raw of [null, undefined, '', 'auto', 'Dark', 'sepia']) expect(parseThemePreference(raw)).toBe('auto');
    expect(readThemePreference(undefined)).toBe('auto');
    expect(
      readThemePreference({
        getItem: () => {
          throw new Error('blocked');
        },
      }),
    ).toBe('auto');
  });

  it('stores light and dark and removes the key for auto', () => {
    const storage = memoryStorage();
    storeThemePreference(storage, 'dark');
    expect(storage.values.get(THEME_STORAGE_KEY)).toBe('dark');
    expect(readThemePreference(storage)).toBe('dark');
    storeThemePreference(storage, 'light');
    expect(readThemePreference(storage)).toBe('light');
    storeThemePreference(storage, 'auto');
    expect(storage.values.has(THEME_STORAGE_KEY)).toBe(false);
    expect(() => {
      storeThemePreference(
        {
          setItem: () => {
            throw new Error('quota');
          },
          removeItem: () => undefined,
        },
        'dark',
      );
    }).not.toThrow();
  });

  it('pins the root scheme through Kerf scope classes across every transition', () => {
    const names = new Set(['app']);
    const root = {
      classList: {
        toggle: (name: string, force: boolean) => {
          if (force) names.add(name);
          else names.delete(name);
          return force;
        },
      },
    } as unknown as Element;
    const classes = () => [...names].filter((name) => name.startsWith('kui-')).sort();
    const walk: [ThemePreference, string[]][] = [
      ['dark', ['kui-dark']],
      ['dark', ['kui-dark']],
      ['light', ['kui-light']],
      ['auto', []],
      ['light', ['kui-light']],
      ['dark', ['kui-dark']],
      ['auto', []],
    ];
    for (const [preference, expected] of walk) {
      applyThemePreference(root, preference);
      expect(classes(), preference).toEqual(expected);
    }
    expect(names.has('app')).toBe(true);
  });
});
