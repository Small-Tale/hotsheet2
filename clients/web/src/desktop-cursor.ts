/**
 * HS2-1RGBCT: standby's hidden cursor in the desktop app. WebKit (and so the macOS app's WKWebView)
 * applies a CSS `cursor` change only when the pointer next moves, so `cursor: none` alone leaves the
 * arrow showing after the Standby button's click and after every rest. The desktop shell's
 * `desktop_hide_cursor_until_moves` command asks AppKit to hide the cursor now, until the mouse moves
 * (`NSCursor setHiddenUntilMouseMoves:`); once it moves, the CSS rule keeps it hidden through a small
 * nudge. The launch-origin capability grants the command to the main window only.
 */
export const DESKTOP_HIDE_CURSOR_COMMAND = 'desktop_hide_cursor_until_moves';

export interface DesktopCursorHost {
  __TAURI_INTERNALS__?: {
    invoke: (command: string, args: { hidden: boolean }) => Promise<unknown>;
  };
}

/**
 * Ask the desktop shell to hide (`true`) or release (`false`) the cursor. A browser (no Tauri
 * internals) does nothing; a refused or failed call is ignored because the CSS rule still applies on
 * the next move.
 */
export async function syncDesktopCursorHidden(
  hidden: boolean,
  host: DesktopCursorHost = window as DesktopCursorHost,
): Promise<void> {
  const internals = host.__TAURI_INTERNALS__;
  if (!internals) return;
  try {
    await internals.invoke(DESKTOP_HIDE_CURSOR_COMMAND, { hidden });
  } catch {
    // Debug builds and older shells lack the command or its permission.
  }
}
