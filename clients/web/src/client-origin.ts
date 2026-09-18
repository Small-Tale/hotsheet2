/** Same-device vs. remote client detection based on the origin the web client was loaded from.
 *
 * A same-device client reaches the server over loopback (`localhost` / `127.0.0.1` / `::1`), so it can
 * browse the server's filesystem to pick a project. A client loaded from any other host (a different
 * device over Tailscale/LAN) cannot, so it must pick from the projects the server already knows about
 * (HS2-VFNCXG). This is deliberately decided from `location.hostname` rather than the server-observed
 * peer address: in dev the browser talks to the Vite proxy, so the hotsheet server would always see
 * loopback regardless of where the real client is. */

/** Whether a hostname is a loopback (same-device) address. */
export function isLoopbackHostname(hostname: string): boolean {
  const host = hostname.trim().replace(/^\[/, '').replace(/\]$/, '').toLowerCase();
  return host === 'localhost' || host === '127.0.0.1' || host === '::1' || host.endsWith('.localhost');
}

/** Whether the web client is running on a different device than the server. Defaults to a non-loopback
 * origin check, but a `?device=remote` / `?device=local` query override forces it — handy for testing
 * and for previewing the remote project picker from the same machine. */
export function isRemoteClient(location: { hostname: string; search?: string } = window.location): boolean {
  const override = new URLSearchParams(location.search ?? '').get('device');
  if (override === 'remote') return true;
  if (override === 'local') return false;
  return !isLoopbackHostname(location.hostname);
}
