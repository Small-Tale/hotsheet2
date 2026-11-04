/**
 * The one place the client opens a new browser window (HS2-0A7FWS).
 *
 * In a browser this is `window.open`. In the desktop app the shell receives the request, opens
 * http(s) and mailto targets in the system browser, and returns no window; a blank popup that a
 * caller means to point somewhere later is refused there, so callers treat `null` as "open the real
 * URL directly once it is known".
 */
export function openExternal(url: string, name?: string, features?: string): Window | null {
  // eslint-disable-next-line no-restricted-properties -- The single window-opening entry point; the desktop shell routes it (HS2-0A7FWS).
  return window.open(url, name, features);
}
