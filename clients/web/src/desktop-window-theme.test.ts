import { describe, expect, it, vi } from 'vitest';

import {
  DESKTOP_MAIN_WINDOW,
  DESKTOP_SET_THEME_COMMAND,
  desktopWindowTheme,
  syncDesktopWindowTheme,
} from './desktop-window-theme';
import type { ThemePreference } from './theme-preference';

// HS2-15W35F: the Tauri window chrome follows App Settings → General → Theme.
describe('desktop window theme bridge (HS2-15W35F)', () => {
  it('maps a pinned scheme to the Tauri theme and auto to the system (null)', () => {
    expect(desktopWindowTheme('light')).toBe('light');
    expect(desktopWindowTheme('dark')).toBe('dark');
    expect(desktopWindowTheme('auto')).toBeNull();
  });

  it('pins and releases the main window theme across every transition', async () => {
    const invoke = vi.fn(() => Promise.resolve());
    const host = { __TAURI_INTERNALS__: { invoke } };
    const walk: ThemePreference[] = ['dark', 'dark', 'light', 'auto', 'light', 'auto'];
    for (const preference of walk) await syncDesktopWindowTheme(preference, host);
    expect(invoke.mock.calls).toEqual(
      walk.map((preference) => [
        DESKTOP_SET_THEME_COMMAND,
        { label: DESKTOP_MAIN_WINDOW, value: preference === 'auto' ? null : preference },
      ]),
    );
    expect(DESKTOP_SET_THEME_COMMAND).toBe('plugin:window|set_theme');
    expect(DESKTOP_MAIN_WINDOW).toBe('main');
  });

  it('does nothing in a browser and ignores a refused native call', async () => {
    await expect(syncDesktopWindowTheme('dark', {})).resolves.toBeUndefined();
    const invoke = vi.fn((): Promise<unknown> => Promise.reject(new Error('core:window:allow-set-theme not allowed')));
    await expect(syncDesktopWindowTheme('dark', { __TAURI_INTERNALS__: { invoke } })).resolves.toBeUndefined();
    expect(invoke).toHaveBeenCalledOnce();
    // A later choice still reaches the shell after a refusal.
    invoke.mockImplementation(() => Promise.resolve());
    await syncDesktopWindowTheme('auto', { __TAURI_INTERNALS__: { invoke } });
    expect(invoke).toHaveBeenLastCalledWith(DESKTOP_SET_THEME_COMMAND, { label: DESKTOP_MAIN_WINDOW, value: null });
  });
});
