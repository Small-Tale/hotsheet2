import { describe, expect, it } from 'vitest';

import {
  DEMO_DISPLAY_STORAGE_KEY,
  readDemoDisplayPreferences,
  writeDemoDisplayPreferences,
} from './display-preferences';

const memory = () => {
  const values = new Map<string, string>();
  return {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => {
      values.set(key, value);
    },
  };
};

describe('UX demo display preferences', () => {
  it('round-trips every display choice', () => {
    const storage = memory();
    const preferences = {
      theme: 'dark',
      increasedContrast: true,
      reducedMotion: false,
      backgroundStyle: 'sunken',
      collapsed: true,
    } as const;
    writeDemoDisplayPreferences(storage, preferences);
    expect(readDemoDisplayPreferences(storage)).toEqual(preferences);
  });

  it('drops malformed values and tolerates unreadable or missing storage', () => {
    const storage = memory();
    storage.setItem(
      DEMO_DISPLAY_STORAGE_KEY,
      JSON.stringify({ theme: 'sepia', increasedContrast: 'yes', backgroundStyle: 'plaid', collapsed: 1 }),
    );
    expect(readDemoDisplayPreferences(storage)).toEqual({
      theme: undefined,
      increasedContrast: undefined,
      reducedMotion: undefined,
      backgroundStyle: undefined,
      collapsed: undefined,
    });
    storage.setItem(DEMO_DISPLAY_STORAGE_KEY, '[1]');
    expect(readDemoDisplayPreferences(storage)).toEqual({});
    storage.setItem(DEMO_DISPLAY_STORAGE_KEY, '{');
    expect(readDemoDisplayPreferences(storage)).toEqual({});
    expect(readDemoDisplayPreferences(null)).toEqual({});
    const blocked = {
      setItem: () => {
        throw new Error('blocked');
      },
    };
    expect(() => {
      writeDemoDisplayPreferences(blocked, { theme: 'light' });
    }).not.toThrow();
  });
});
