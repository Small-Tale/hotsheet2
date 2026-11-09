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
  const value = (await response.json()) as T & { error?: string };
  if (!response.ok) throw new Error(value.error || `Remote request failed (${response.status}).`);
  return value;
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
