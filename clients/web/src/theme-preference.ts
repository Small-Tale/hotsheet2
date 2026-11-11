/**
 * App Settings → General → Theme (HS2-EFTHS6): this browser's appearance choice. `auto` (the default)
 * follows the system's `prefers-color-scheme`; `light` and `dark` pin the scheme through Kerf's
 * root appearance scope classes (`.kui-light` / `.kui-dark`), which set `color-scheme` for every Kerf
 * and Web Awesome token. The choice is per browser, like the other General settings.
 */
export type ThemePreference = 'auto' | 'light' | 'dark';

export const THEME_PREFERENCES: readonly ThemePreference[] = ['auto', 'light', 'dark'];

/** The browser storage key; listed in `SETTINGS_STORAGE` (settings-navigation). */
export const THEME_STORAGE_KEY = 'hotsheet.theme';

/** A stored value as a preference; anything unknown or missing is `auto`. */
export function parseThemePreference(raw: string | null | undefined): ThemePreference {
  return raw === 'light' || raw === 'dark' ? raw : 'auto';
}

/** The stored preference, or `auto` when storage is missing or refuses the read. */
export function readThemePreference(storage: Pick<Storage, 'getItem'> | undefined): ThemePreference {
  try {
    return parseThemePreference(storage?.getItem(THEME_STORAGE_KEY));
  } catch {
    return 'auto';
  }
}

/** Store the preference; `auto` removes the key. A refused write leaves the session's choice in place. */
export function storeThemePreference(
  storage: Pick<Storage, 'setItem' | 'removeItem'> | undefined,
  preference: ThemePreference,
): void {
  try {
    if (preference === 'auto') storage?.removeItem(THEME_STORAGE_KEY);
    else storage?.setItem(THEME_STORAGE_KEY, preference);
  } catch {
    // Private windows and blocked storage keep the choice for this page only.
  }
}

/** Pin or release the document's color scheme through Kerf's appearance scope classes. */
export function applyThemePreference(root: Element, preference: ThemePreference): void {
  root.classList.toggle('kui-light', preference === 'light');
  root.classList.toggle('kui-dark', preference === 'dark');
}
