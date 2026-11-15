import { describe, expect, it } from 'vitest';

import {
  applyThemePreference,
  parseThemePreference,
  readThemePreference,
  storeThemePreference,
  THEME_COLORS,
  THEME_STORAGE_KEY,
  themeColorFor,
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
      ownerDocument: { querySelectorAll: () => [] },
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

  // HS2-15W35F: the PWA theme-color metas follow a pinned scheme and return to their media under Auto.
  it('keeps both theme-color media variants on the pinned scheme and restores them under auto', () => {
    const metas = ['(prefers-color-scheme: light)', '(prefers-color-scheme: dark)'].map((media) => {
      const attributes = new Map([
        ['media', media],
        ['content', 'stale'],
      ]);
      return {
        getAttribute: (name: string) => attributes.get(name) ?? null,
        setAttribute: (name: string, value: string) => void attributes.set(name, value),
      };
    });
    let selector = '';
    const root = {
      classList: { toggle: () => true },
      ownerDocument: {
        querySelectorAll: (query: string) => {
          selector = query;
          return metas;
        },
      },
    } as unknown as Element;
    const contents = () => metas.map((meta) => meta.getAttribute('content'));
    const walk: [ThemePreference, string[]][] = [
      ['auto', [THEME_COLORS.light, THEME_COLORS.dark]],
      ['dark', [THEME_COLORS.dark, THEME_COLORS.dark]],
      ['dark', [THEME_COLORS.dark, THEME_COLORS.dark]],
      ['light', [THEME_COLORS.light, THEME_COLORS.light]],
      ['auto', [THEME_COLORS.light, THEME_COLORS.dark]],
      ['light', [THEME_COLORS.light, THEME_COLORS.light]],
      ['auto', [THEME_COLORS.light, THEME_COLORS.dark]],
    ];
    for (const [preference, expected] of walk) {
      applyThemePreference(root, preference);
      expect(contents(), preference).toEqual(expected);
    }
    expect(selector).toBe('meta[name="theme-color"]');
    expect(metas.map((meta) => meta.getAttribute('media'))).toEqual([
      '(prefers-color-scheme: light)',
      '(prefers-color-scheme: dark)',
    ]);
    // A meta without a media query is the light default under Auto.
    expect(themeColorFor('', 'auto')).toBe(THEME_COLORS.light);
    expect(themeColorFor('', 'dark')).toBe(THEME_COLORS.dark);
  });
});
