import { describe, expect, it, vi } from 'vitest';

import { DESKTOP_HIDE_CURSOR_COMMAND, syncDesktopCursorHidden } from './desktop-cursor';

// HS2-1RGBCT: WebKit repaints a CSS cursor change only on the next move, so the shell hides it.
describe('desktop cursor bridge (HS2-1RGBCT)', () => {
  it('asks the shell to hide and release the cursor across every transition', async () => {
    const invoke = vi.fn(() => Promise.resolve());
    const host = { __TAURI_INTERNALS__: { invoke } };
    const walk = [true, false, true, true, false];
    for (const hidden of walk) await syncDesktopCursorHidden(hidden, host);
    expect(invoke.mock.calls).toEqual(walk.map((hidden) => [DESKTOP_HIDE_CURSOR_COMMAND, { hidden }]));
    expect(DESKTOP_HIDE_CURSOR_COMMAND).toBe('desktop_hide_cursor_until_moves');
  });

  it('does nothing in a browser and ignores a refused native call', async () => {
    await expect(syncDesktopCursorHidden(true, {})).resolves.toBeUndefined();
    const invoke = vi.fn((): Promise<unknown> => Promise.reject(new Error('not allowed')));
    await expect(syncDesktopCursorHidden(true, { __TAURI_INTERNALS__: { invoke } })).resolves.toBeUndefined();
    expect(invoke).toHaveBeenCalledOnce();
    // A later change still reaches the shell after a refusal.
    invoke.mockImplementation(() => Promise.resolve());
    await syncDesktopCursorHidden(false, { __TAURI_INTERNALS__: { invoke } });
    expect(invoke).toHaveBeenLastCalledWith(DESKTOP_HIDE_CURSOR_COMMAND, { hidden: false });
  });
});
