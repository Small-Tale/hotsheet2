import type { ThemePreference } from './theme-preference';

/**
 * HS2-15W35F: the desktop window's native chrome (macOS title bar and traffic-light area) follows
 * App Settings → General → Theme. Tauri's core `set_theme` takes `light`, `dark`, or `null`, and
 * `null` returns the window to the system appearance, which is what Auto means. The packaged app's
 * launch-origin capability grants `core:window:allow-set-theme` to the main window only.
 */
export const DESKTOP_SET_THEME_COMMAND = 'plugin:window|set_theme';
export const DESKTOP_MAIN_WINDOW = 'main';

export interface DesktopWindowThemeHost {
  __TAURI_INTERNALS__?: {
    invoke: (command: string, args: { label: string; value: 'light' | 'dark' | null }) => Promise<unknown>;
  };
}

/** The Tauri theme value for a preference; `null` follows the system. */
export function desktopWindowTheme(preference: ThemePreference): 'light' | 'dark' | null {
  return preference === 'auto' ? null : preference;
}

/**
 * Ask the desktop shell to pin or release the window theme. A browser (no Tauri internals) does
 * nothing; a refused or failed call is ignored because the chrome color is cosmetic and the
 * document's own scheme is already applied.
 */
export async function syncDesktopWindowTheme(
  preference: ThemePreference,
  host: DesktopWindowThemeHost = window as DesktopWindowThemeHost,
): Promise<void> {
  const internals = host.__TAURI_INTERNALS__;
  if (!internals) return;
  try {
    await internals.invoke(DESKTOP_SET_THEME_COMMAND, {
      label: DESKTOP_MAIN_WINDOW,
      value: desktopWindowTheme(preference),
    });
  } catch {
    // Debug builds and older shells lack the permission; the window keeps the system appearance.
  }
}
