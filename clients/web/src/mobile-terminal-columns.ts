/** Column counts a phone-width terminal can use; fewer columns means larger text (HS2-WMN626). */
export const MOBILE_TERMINAL_COLUMN_OPTIONS = [80, 70, 60, 50, 40] as const;
export const DEFAULT_MOBILE_TERMINAL_COLUMNS = MOBILE_TERMINAL_COLUMN_OPTIONS[0];
export const MOBILE_TERMINAL_COLUMNS_STORAGE_KEY = 'hotsheet.terminals.mobile-columns';
/** Window event asking mounted mobile terminals to refit after the column preference changes. */
export const MOBILE_TERMINAL_COLUMNS_CHANGE_EVENT = 'hotsheet-mobile-terminal-columns-change';

export function normalizeMobileTerminalColumns(value: unknown): number {
  const parsed = typeof value === 'string' ? Number(value) : value;
  return MOBILE_TERMINAL_COLUMN_OPTIONS.find((option) => option === parsed) ?? DEFAULT_MOBILE_TERMINAL_COLUMNS;
}

/** Cycle 80 → 70 → 60 → 50 → 40 → 80. */
export function nextMobileTerminalColumns(current: number): number {
  const options: readonly number[] = MOBILE_TERMINAL_COLUMN_OPTIONS,
    index = options.indexOf(normalizeMobileTerminalColumns(current));
  return options[(index + 1) % options.length];
}

export function loadMobileTerminalColumns(storage: Pick<Storage, 'getItem'>): number {
  try {
    return normalizeMobileTerminalColumns(storage.getItem(MOBILE_TERMINAL_COLUMNS_STORAGE_KEY));
  } catch {
    return DEFAULT_MOBILE_TERMINAL_COLUMNS;
  }
}

export function saveMobileTerminalColumns(storage: Pick<Storage, 'setItem'>, value: number): number {
  const normalized = normalizeMobileTerminalColumns(value);
  try {
    storage.setItem(MOBILE_TERMINAL_COLUMNS_STORAGE_KEY, String(normalized));
  } catch {
    /* storage can be unavailable in private browsing; the in-memory preference still applies */
  }
  return normalized;
}
