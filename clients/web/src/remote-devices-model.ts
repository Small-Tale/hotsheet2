/**
 * The host's paired and pending browser devices as `/remote-auth/owner` reports them (HS2-DXM0YN,
 * docs/contracts/v1/openapi.yaml `RemoteDeviceList`). Parsed explicitly so a changed server answer
 * fails here, not at a later property read.
 */

export type RemoteDeviceRole = 'read_only' | 'read_write';

export interface RemoteDevice {
  id: string;
  label: string;
  role: RemoteDeviceRole;
  /** May manage other devices. */
  admin: boolean;
  /** Host-trusted: the owner's own browser (HS2-85W6BE). */
  local: boolean;
  origin: string;
  created_at: number;
  last_used_at: number;
  expires_at: number;
}

export interface RemotePendingDevice {
  id: string;
  label: string;
  origin: string;
  created_at: number;
  expires_at: number;
  /** Approved but not yet redeemed by its browser. */
  approved: boolean;
}

export interface RemoteDeviceList {
  server_id: string;
  revision: number;
  pair_ttl_seconds: number;
  /** Who is looking: the trusted host (no device id) or one admin session. */
  manager: { device_id?: string; local: boolean };
  devices: RemoteDevice[];
  pending: RemotePendingDevice[];
}

/** A partial change to a paired device; omitted values stay. */
export interface RemoteDeviceChange {
  role?: RemoteDeviceRole;
  admin?: boolean;
  local?: boolean;
}

class RemoteDeviceListError extends Error {
  constructor(what: string) {
    super(`The host returned an invalid device list (${what}).`);
    this.name = 'RemoteDeviceListError';
  }
}

const record = (value: unknown, what: string): Record<string, unknown> => {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) throw new RemoteDeviceListError(what);
  return value as Record<string, unknown>;
};
const text = (value: Record<string, unknown>, key: string, what: string): string => {
  const field = value[key];
  if (typeof field !== 'string') throw new RemoteDeviceListError(`${what}.${key}`);
  return field;
};
const number = (value: Record<string, unknown>, key: string, what: string): number => {
  const field = value[key];
  if (typeof field !== 'number' || !Number.isFinite(field)) throw new RemoteDeviceListError(`${what}.${key}`);
  return field;
};
const flag = (value: Record<string, unknown>, key: string, what: string): boolean => {
  const field = value[key];
  if (typeof field !== 'boolean') throw new RemoteDeviceListError(`${what}.${key}`);
  return field;
};
const role = (value: Record<string, unknown>, what: string): RemoteDeviceRole => {
  const field = value.role;
  if (field !== 'read_only' && field !== 'read_write') throw new RemoteDeviceListError(`${what}.role`);
  return field;
};
const list = (value: Record<string, unknown>, key: string): unknown[] => {
  const field = value[key];
  if (!Array.isArray(field)) throw new RemoteDeviceListError(key);
  return field;
};

export function parseRemoteDeviceList(value: unknown): RemoteDeviceList {
  const body = record(value, 'body'),
    manager = record(body.manager, 'manager'),
    deviceId = manager.device_id;
  if (deviceId !== undefined && typeof deviceId !== 'string') throw new RemoteDeviceListError('manager.device_id');
  return {
    server_id: text(body, 'server_id', 'body'),
    revision: number(body, 'revision', 'body'),
    pair_ttl_seconds: number(body, 'pair_ttl_seconds', 'body'),
    manager: { ...(deviceId === undefined ? {} : { device_id: deviceId }), local: flag(manager, 'local', 'manager') },
    devices: list(body, 'devices').map((item, index) => {
      const what = `devices[${index}]`,
        entry = record(item, what);
      return {
        id: text(entry, 'id', what),
        label: text(entry, 'label', what),
        role: role(entry, what),
        admin: flag(entry, 'admin', what),
        local: flag(entry, 'local', what),
        origin: text(entry, 'origin', what),
        created_at: number(entry, 'created_at', what),
        last_used_at: number(entry, 'last_used_at', what),
        expires_at: number(entry, 'expires_at', what),
      };
    }),
    pending: list(body, 'pending').map((item, index) => {
      const what = `pending[${index}]`,
        entry = record(item, what);
      return {
        id: text(entry, 'id', what),
        label: text(entry, 'label', what),
        origin: text(entry, 'origin', what),
        created_at: number(entry, 'created_at', what),
        expires_at: number(entry, 'expires_at', what),
        approved: flag(entry, 'approved', what),
      };
    }),
  };
}

/** The description line of a paired device row: its access, then when it was last used. The origin
 * stays out of the line so a narrow row keeps the access visible. */
export function remoteDeviceSummary(device: RemoteDevice, formatTime: (seconds: number) => string): string {
  const access = [
    device.role === 'read_write' ? 'Read-write' : 'Read-only',
    ...(device.admin ? ['administrator'] : []),
    ...(device.local ? ['host access'] : []),
  ].join(', ');
  return `${access} · last used ${formatTime(device.last_used_at)}`;
}

/** The description line of a pending request row: the request id the owner compares with the
 * requesting browser first, then its expiry and origin. */
export function pendingDeviceSummary(pending: RemotePendingDevice, formatTime: (seconds: number) => string): string {
  return pending.approved
    ? `Request ${pending.id} · waiting for ${pending.origin} to finish pairing`
    : `Request ${pending.id} · expires ${formatTime(pending.expires_at)} · ${pending.origin}`;
}

/** Clock time for a Unix timestamp in seconds, in the browser's locale. */
export function deviceTime(seconds: number): string {
  return new Date(seconds * 1000).toLocaleString(undefined, {
    month: 'short',
    day: 'numeric',
    hour: 'numeric',
    minute: '2-digit',
  });
}
