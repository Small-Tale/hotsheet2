/** Browser-facing metadata only. Session tokens, CSRF proof and certificate material never cross
 * this client boundary or enter browser storage (HS2-BT4KN6). */
import type { Project } from '../interactions/types';

export interface DesktopRemoteServer {
  id: string;
  label: string;
  origin: string;
  identity: { server_id: string; ca_fingerprint: string | null };
  revision: number;
  revoked: boolean;
  last_failure: string | null;
  stale: boolean;
}

export interface DesktopRemoteCheckout {
  id: string;
  name: string;
  state: 'available' | 'inaccessible' | 'identity_changed';
}

export interface DesktopRemoteDiscovery {
  remote: DesktopRemoteServer;
  checkouts: DesktopRemoteCheckout[] | null;
}

export interface DesktopRemotePairing {
  handle: string;
  approvalId: string;
  serverId: string;
  origin: string;
  label: string;
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const headers = new Headers(init?.headers);
  headers.set('content-type', 'application/json');
  const response = await fetch(`/__hotsheet/remotes${path}`, {
    ...init,
    headers,
  });
  // Parse defensively: a plain-text or HTML reply must never surface a parser message such as
  // WebKit's "The string did not match the expected pattern." (HS2-HBM3JN).
  const text = await response.text();
  let value: unknown;
  try {
    value = text ? JSON.parse(text) : undefined;
  } catch {
    value = undefined;
  }
  if (!response.ok) {
    const error = isRecord(value) && typeof value.error === 'string' ? value.error : '';
    throw new Error(error || remoteRequestFailure(response.status));
  }
  if (value === undefined) throw new Error('This Hot Sheet host sent an unexpected reply. Reload and try again.');
  return value as T;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null;
}

/** A readable message when the host's error has no JSON body. */
export function remoteRequestFailure(status: number): string {
  if (status === 404) return 'This Hot Sheet host does not let this browser manage remote servers.';
  if (status === 401) return 'Sign in to this Hot Sheet host again, then retry.';
  if (status === 403) return 'This Hot Sheet host refused the request. Reload and try again.';
  return `Remote request failed (${status}).`;
}

export const desktopRemoteServers = () => request<DesktopRemoteServer[]>('');
export const desktopRemoteSelected = () =>
  request<Array<{ binding_id: string; checkout_id: string }>>('/selected-projects');
export const desktopRemoteDiscover = (id: string) =>
  request<DesktopRemoteDiscovery>(`/${encodeURIComponent(id)}/checkouts`);
export const desktopRemoteBeginPair = (
  address: string,
  label: string,
  tls: { caFile?: string; certificateFile?: string; keyFile?: string } = {},
  reviewId?: string,
) =>
  request<DesktopRemotePairing>('/pair', { method: 'POST', body: JSON.stringify({ address, label, tls, reviewId }) });
export const desktopRemoteFinishPair = (handle: string) =>
  request<DesktopRemoteDiscovery>(`/pair/${encodeURIComponent(handle)}/redeem`, { method: 'POST' });
export const desktopRemoteOpen = (id: string, checkout: string) =>
  request<Project>(`/${encodeURIComponent(id)}/projects/${encodeURIComponent(checkout)}/open`, { method: 'POST' });
export const desktopRemoteForget = (id: string, checkout: string) =>
  request<{ selected: boolean }>(`/${encodeURIComponent(id)}/projects/${encodeURIComponent(checkout)}`, {
    method: 'DELETE',
  });
export const desktopRemoteRemove = (id: string, revision: number) =>
  request<DesktopRemoteServer>(`/${encodeURIComponent(id)}?revision=${revision}`, { method: 'DELETE' });
