import { describe, expect, it } from 'vitest';

import {
  DEFAULT_MOBILE_TERMINAL_COLUMNS,
  loadMobileTerminalColumns,
  MOBILE_TERMINAL_COLUMN_OPTIONS,
  MOBILE_TERMINAL_COLUMNS_STORAGE_KEY,
  nextMobileTerminalColumns,
  normalizeMobileTerminalColumns,
  saveMobileTerminalColumns,
} from './mobile-terminal-columns';

describe('mobile terminal columns (HS2-WMN626)', () => {
  it('offers 80 down to 40 columns in steps of 10', () => {
    expect(MOBILE_TERMINAL_COLUMN_OPTIONS).toEqual([80, 70, 60, 50, 40]);
    expect(DEFAULT_MOBILE_TERMINAL_COLUMNS).toBe(80);
  });

  it('cycles through every option and wraps back to 80', () => {
    const seen = [80];
    for (let index = 0; index < 5; index++) seen.push(nextMobileTerminalColumns(seen.at(-1)!));
    expect(seen).toEqual([80, 70, 60, 50, 40, 80]);
  });

  it('normalizes unknown, fractional, and string values', () => {
    expect(normalizeMobileTerminalColumns('60')).toBe(60);
    expect(normalizeMobileTerminalColumns(65)).toBe(80);
    expect(normalizeMobileTerminalColumns(null)).toBe(80);
    expect(normalizeMobileTerminalColumns('abc')).toBe(80);
    expect(nextMobileTerminalColumns(33)).toBe(70);
  });

  it('persists, reloads, resets, and edits again', () => {
    const values = new Map<string, string>(),
      storage = {
        getItem: (key: string) => values.get(key) ?? null,
        setItem: (key: string, value: string) => void values.set(key, value),
      };
    expect(loadMobileTerminalColumns(storage)).toBe(80);
    expect(saveMobileTerminalColumns(storage, 50)).toBe(50);
    expect(values.get(MOBILE_TERMINAL_COLUMNS_STORAGE_KEY)).toBe('50');
    expect(loadMobileTerminalColumns(storage)).toBe(50);
    values.set(MOBILE_TERMINAL_COLUMNS_STORAGE_KEY, 'garbage');
    expect(loadMobileTerminalColumns(storage)).toBe(80);
    expect(saveMobileTerminalColumns(storage, nextMobileTerminalColumns(loadMobileTerminalColumns(storage)))).toBe(70);
    expect(loadMobileTerminalColumns(storage)).toBe(70);
  });

  it('survives unavailable storage', () => {
    const broken = {
      getItem: () => {
        throw new Error('denied');
      },
      setItem: () => {
        throw new Error('denied');
      },
    };
    expect(loadMobileTerminalColumns(broken)).toBe(80);
    expect(saveMobileTerminalColumns(broken, 40)).toBe(40);
  });
});
