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

import { signal } from 'kerfjs';

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

/** A signal, so every render and effect that asked about host actions or device management follows a
 * grant changed elsewhere without a reload (HS2-A3TDNH). */
const sessionGrants = signal<BrowserSessionGrants | undefined>(undefined);

/** The lifetime of this browser's paired session, as `/remote-auth/me` last reported it (HS2-4B0KSD). */
export interface BrowserSessionLifetime {
  renewed_at: number;
  expires_at: number;
  idle_expires_at: number;
  renewal_requested_at: number | null;
}

const sessionLifetime = signal<BrowserSessionLifetime | undefined>(undefined);

/** This page's session lifetime; undefined before it is known or on a local-mode host. */
export function browserSessionLifetime(): BrowserSessionLifetime | undefined {
  return sessionLifetime.value;
}

function recordLifetime(record: Record<string, unknown>) {
  const { renewed_at, expires_at, idle_expires_at, renewal_requested_at } = record;
  if (typeof renewed_at !== 'number' || typeof expires_at !== 'number' || typeof idle_expires_at !== 'number') return;
  const next = {
      renewed_at,
      expires_at,
      idle_expires_at,
      renewal_requested_at: typeof renewal_requested_at === 'number' ? renewal_requested_at : null,
    },
    current = sessionLifetime.peek();
  if (
    current?.renewed_at === next.renewed_at &&
    current.expires_at === next.expires_at &&
    current.idle_expires_at === next.idle_expires_at &&
    current.renewal_requested_at === next.renewal_requested_at
  )
    return;
  sessionLifetime.value = next;
}

/** Record the grants `/remote-auth/me` (or a redeem) reported for this page's session. Unchanged grants
 * leave the signal alone, so a re-read that finds nothing new re-renders nothing. The answer's lifetime
 * is recorded too (HS2-4B0KSD). */
export function setBrowserSessionGrants(device: unknown): BrowserSessionGrants {
  const record = typeof device === 'object' && device !== null ? (device as Record<string, unknown>) : {};
  recordLifetime(record);
  const next = { local: record.local === true, admin: record.admin === true },
    current = sessionGrants.peek();
  if (current?.local === next.local && current.admin === next.admin) return current;
  sessionGrants.value = next;
  return next;
}

/** This page's session grants; undefined before the session is known or on a local-mode host. A read
 * inside a render or effect subscribes it to grant changes. */
export function browserSessionGrants(): BrowserSessionGrants | undefined {
  return sessionGrants.value;
}

/** Forget the session grants (tests and a signed-out page). */
export function resetBrowserSessionGrants(): void {
  sessionGrants.value = undefined;
  sessionLifetime.value = undefined;
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
  if (doc && stampedBrowserAuthMode(doc) === 'remote') return sessionGrants.value?.local !== true;
  return !isLoopbackHostname(location.hostname);
}

/** Whether this client may manage remote devices (HS2-DXM0YN): a local-mode host's own browser is the
 * trusted owner; on a remote-mode host only an admin session may. */
export function canManageRemoteDevices(
  doc: Pick<Document, 'querySelector'> | undefined = typeof document === 'undefined' ? undefined : document,
): boolean {
  if (!doc || stampedBrowserAuthMode(doc) !== 'remote') return true;
  return sessionGrants.value?.admin === true;
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
