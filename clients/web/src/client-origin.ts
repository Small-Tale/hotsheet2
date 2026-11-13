/** Same-device vs. remote client detection based on the origin the web client was loaded from.
 *
 * A same-device client reaches the server over loopback (`localhost` / `127.0.0.1` / `::1`), so it can
 * browse the server's filesystem to pick a project. A client loaded from any other host (a different
 * device over Tailscale/LAN) cannot, so it must pick from the projects the server already knows about
 * (HS2-VFNCXG). On a local-mode host this is decided from `location.hostname` rather than the
 * server-observed peer address: in dev the browser talks to the Vite proxy, so the hotsheet server would
 * always see loopback regardless of where the real client is. On a remote-mode host neither can decide,
 * because a TLS proxy such as Tailscale Serve also arrives over loopback; the paired session's `local`
 * grant decides instead (HS2-85W6BE). */

/** Whether a hostname is a loopback (same-device) address. */
export function isLoopbackHostname(hostname: string): boolean {
  const host = hostname.trim().replace(/^\[/, '').replace(/\]$/, '').toLowerCase();
  return host === 'localhost' || host === '127.0.0.1' || host === '::1' || host.endsWith('.localhost');
}

/** The grants of this browser's paired session on a remote-mode host (HS2-85W6BE, HS2-DXM0YN). */
export interface BrowserSessionGrants {
  /** Host-trusted: the owner's own browser, which may use host-only actions. */
  local: boolean;
  /** May manage other devices. */
  admin: boolean;
}

let sessionGrants: BrowserSessionGrants | undefined;

/** Record the grants `/remote-auth/me` (or a redeem) reported for this page's session. */
export function setBrowserSessionGrants(device: unknown): BrowserSessionGrants {
  const record = typeof device === 'object' && device !== null ? (device as Record<string, unknown>) : {};
  sessionGrants = { local: record.local === true, admin: record.admin === true };
  return sessionGrants;
}

/** This page's session grants; undefined before the session is known or on a local-mode host. */
export function browserSessionGrants(): BrowserSessionGrants | undefined {
  return sessionGrants;
}

/** Forget the session grants (tests and a signed-out page). */
export function resetBrowserSessionGrants(): void {
  sessionGrants = undefined;
}

/** Whether the web client is running on a different device than the server. A `?device=remote` /
 * `?device=local` query override forces it, which is handy for testing and for previewing the remote
 * project picker from the same machine. On a remote-mode host the session's `local` grant decides
 * (HS2-85W6BE): Tailscale Serve reaches that host through the same loopback port the owner opens, so
 * the hostname cannot. Otherwise a non-loopback hostname means another device. */
export function isRemoteClient(
  location: { hostname: string; search?: string } = window.location,
  doc: Pick<Document, 'querySelector'> | undefined = typeof document === 'undefined' ? undefined : document,
): boolean {
  const override = new URLSearchParams(location.search ?? '').get('device');
  if (override === 'remote') return true;
  if (override === 'local') return false;
  if (doc && stampedBrowserAuthMode(doc) === 'remote') return sessionGrants?.local !== true;
  return !isLoopbackHostname(location.hostname);
}

/** Whether this client may manage remote devices (HS2-DXM0YN): a local-mode host's own browser is the
 * trusted owner; on a remote-mode host only an admin session may. */
export function canManageRemoteDevices(
  doc: Pick<Document, 'querySelector'> | undefined = typeof document === 'undefined' ? undefined : document,
): boolean {
  if (!doc || stampedBrowserAuthMode(doc) !== 'remote') return true;
  return sessionGrants?.admin === true;
}

/** The browser-auth mode the serving host stamped into the document (HS2-7DBJMS), if any. */
export function stampedBrowserAuthMode(
  doc: Pick<Document, 'querySelector'> = document,
): 'local' | 'remote' | undefined {
  const mode = doc.querySelector<HTMLMetaElement>('meta[name="hotsheet-browser-auth"]')?.content;
  return mode === 'local' || mode === 'remote' ? mode : undefined;
}

/** Whether project opens must use the registered-checkout remote route (HS2-3HC9HT): a remote device
 * never browses the host's folders. On a remote-mode host only a session with the `local` grant opens
 * by path, whatever hostname reached it (HS2-85W6BE). */
export function opensProjectsRemotely(): boolean {
  if (typeof document === 'undefined') return false;
  return isRemoteClient();
}
