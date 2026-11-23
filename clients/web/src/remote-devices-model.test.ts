import { describe, expect, it } from 'vitest';

import {
  parseRemoteDeviceList,
  pendingDeviceSummary,
  remoteDeviceLifetime,
  remoteDeviceLifetimeState,
  remoteDeviceSummary,
} from './remote-devices-model';

/** The server's `/remote-auth/owner` shape (crates/hotsheet-server/src/routes/remote_auth.rs `DeviceList`). */
const SERVER_DEVICE_LIST = {
  server_id: 'synthetic-server',
  revision: 1_794_000_000_123,
  pair_ttl_seconds: 900,
  manager: { device_id: 'D1', local: false },
  devices: [
    {
      id: 'D1',
      server_id: 'synthetic-server',
      label: 'Chrome on macOS',
      role: 'read_write',
      admin: true,
      local: false,
      origin: 'http://127.0.0.1:4175',
      created_at: 100,
      last_used_at: 200,
      expires_at: 300,
    },
  ],
  pending: [
    { id: 'P1', label: 'Safari on iPhone', origin: 'https://h.example', created_at: 1, expires_at: 2, approved: false },
  ],
};

describe('HS2-4B0KSD device lifetime in the list', () => {
  const lifetime = {
    renewed_at: 100,
    idle_expires_at: 250,
    renewal_requested_at: null,
    reminder_day: null,
    expired: false,
  };
  const device = (overrides: Record<string, unknown> = {}) =>
    parseRemoteDeviceList({
      ...SERVER_DEVICE_LIST,
      devices: [{ ...SERVER_DEVICE_LIST.devices[0], ...lifetime, ...overrides }],
    }).devices[0];
  const at = (seconds: number) => `t${String(seconds)}`;

  it('reads the lifetime and names each state, ending at the earlier clock', () => {
    expect(device()).toMatchObject(lifetime);
    expect(remoteDeviceLifetimeState(device())).toBe('active');
    expect(remoteDeviceLifetime(device(), at)).toBe('expires t250');
    expect(remoteDeviceLifetimeState(device({ reminder_day: 70 }))).toBe('reminder');
    expect(remoteDeviceLifetimeState(device({ renewal_requested_at: 240, reminder_day: 70 }))).toBe(
      'renewal_requested',
    );
    expect(remoteDeviceLifetime(device({ renewal_requested_at: 240 }), at)).toBe('asks to be renewed · expires t250');
    const expired = device({ expired: true, renewal_requested_at: 240 });
    expect(remoteDeviceLifetimeState(expired)).toBe('expired');
    expect(remoteDeviceLifetime(expired, at)).toBe('approval expired t250; renew to restore it');
  });

  it('reads a host from before the lifetime fields as counted from approval', () => {
    expect(parseRemoteDeviceList(SERVER_DEVICE_LIST).devices[0]).toMatchObject({
      renewed_at: 100,
      idle_expires_at: 300,
      renewal_requested_at: null,
      reminder_day: null,
      expired: false,
    });
    expect(() => device({ expired: 'yes' })).toThrow(/invalid device list/);
    expect(() => device({ reminder_day: 'soon' })).toThrow(/invalid device list/);
  });
});

describe('HS2-DXM0YN remote device list contract', () => {
  it('parses the server answer and keeps an absent manager device for the trusted host', () => {
    const parsed = parseRemoteDeviceList(SERVER_DEVICE_LIST);
    expect(parsed.manager).toEqual({ device_id: 'D1', local: false });
    expect(parsed.devices[0]).toMatchObject({ id: 'D1', role: 'read_write', admin: true, local: false });
    expect(parsed.pending[0]?.approved).toBe(false);
    const host = parseRemoteDeviceList({ ...SERVER_DEVICE_LIST, manager: { local: true } });
    expect(host.manager).toEqual({ local: true });
  });

  it.each([
    ['a missing grant', { ...SERVER_DEVICE_LIST, devices: [{ ...SERVER_DEVICE_LIST.devices[0], admin: undefined }] }],
    ['an unknown role', { ...SERVER_DEVICE_LIST, devices: [{ ...SERVER_DEVICE_LIST.devices[0], role: 'owner' }] }],
    ['a missing cursor', { ...SERVER_DEVICE_LIST, revision: 'next' }],
    ['a numeric manager id', { ...SERVER_DEVICE_LIST, manager: { device_id: 7, local: true } }],
    ['no pending list', { ...SERVER_DEVICE_LIST, pending: undefined }],
    ['a pending entry without approval state', { ...SERVER_DEVICE_LIST, pending: [{ id: 'P1' }] }],
    ['an array body', []],
  ])('refuses %s', (_reason, body) => {
    expect(() => parseRemoteDeviceList(body)).toThrow(/invalid device list/);
  });

  it('describes access in words, not color alone', () => {
    const device = parseRemoteDeviceList(SERVER_DEVICE_LIST).devices[0];
    const at = (seconds: number) => `t${String(seconds)}`;
    expect(remoteDeviceSummary(device, at)).toBe('Read-write, administrator · last used t200 · expires t300');
    expect(remoteDeviceSummary({ ...device, role: 'read_only', admin: false, local: true }, at)).toBe(
      'Read-only, host access · last used t200 · expires t300',
    );
    const pending = parseRemoteDeviceList(SERVER_DEVICE_LIST).pending[0];
    expect(pendingDeviceSummary(pending, at)).toBe('Request P1 · expires t2 · https://h.example');
    expect(pendingDeviceSummary({ ...pending, approved: true }, at)).toBe(
      'Request P1 · waiting for https://h.example to finish pairing',
    );
  });
});
