import { describe, expect, it } from 'vitest';

import type { RemoteDeviceList } from '../remote-devices-model';
import { RemoteDevicesSettings } from './remote-devices-settings';

const at = (seconds: number) => `t${String(seconds)}`;
const fixture = (manager: RemoteDeviceList['manager']): RemoteDeviceList => ({
  server_id: 'synthetic-server',
  revision: 1,
  pair_ttl_seconds: 900,
  manager,
  pending: [
    { id: 'P1', label: 'Safari on iPhone', origin: 'https://h.example', created_at: 1, expires_at: 2, approved: false },
  ],
  devices: [
    {
      id: 'D1',
      label: 'Chrome on macOS',
      role: 'read_write',
      admin: true,
      local: true,
      origin: 'http://127.0.0.1:4175',
      created_at: 1,
      last_used_at: 5,
      expires_at: 9,
    },
    {
      id: 'D2',
      label: 'Safari on iPad',
      role: 'read_only',
      admin: false,
      local: false,
      origin: 'https://h.example',
      created_at: 1,
      last_used_at: 6,
      expires_at: 9,
    },
  ],
});
const render = (props: Parameters<typeof RemoteDevicesSettings>[0]) => String(RemoteDevicesSettings(props));

describe('HS2-DXM0YN App Settings → Devices', () => {
  it('lists requests and devices with every command in each row menu', () => {
    const markup = render({ state: 'ready', list: fixture({ local: true }), formatTime: at });
    expect(markup).toContain('A pairing request expires after 15 minutes.');
    expect(markup).toContain('data-remote-pending="P1"');
    expect(markup).toContain('Request P1 · expires t2 · https://h.example');
    for (const command of [
      'Approve Read-Write',
      'Approve Read-Only',
      'Reject',
      'Make Read-Only',
      'Allow Changes',
      'Administrator',
      'Host Access',
      'Revoke',
    ])
      expect(markup, command).toContain(command);
    expect(markup).toContain('data-action="approve-remote-device"');
    expect(markup).toContain('data-role="read_write"');
    expect(markup).toContain('data-device-id="D2"');
    expect(markup).toContain('Read-write, administrator, host access · last used t5');
    expect(markup).not.toContain('Only the host or a device with host access');
    expect(markup).not.toContain('This Device');
  });

  it('HS2-KTHQE5 offers Host Access to an administrator without host access, and marks its own row', () => {
    const markup = render({ state: 'ready', list: fixture({ device_id: 'D2', local: false }), formatTime: at });
    expect(markup).toContain('Host Access');
    expect(markup).not.toContain('Only the host or a device with host access');
    expect(markup).toContain('This Device');
    expect(markup).toContain('Revoke This Device');
    // The manager's own host access no longer changes what it may do.
    expect(markup).toBe(render({ state: 'ready', list: fixture({ device_id: 'D2', local: true }), formatTime: at }));
  });

  it('distinguishes loading, empty, refused, and failed states', () => {
    const loading = render({ state: 'loading' });
    expect(loading).toContain('data-placeholder');
    expect(loading).not.toContain('No pairing requests.');

    const empty = render({ state: 'ready', list: { ...fixture({ local: true }), pending: [], devices: [] } });
    expect(empty).toContain('No pairing requests.');
    expect(empty).toContain('No paired devices.');

    const refused = render({ state: 'forbidden' });
    expect(refused).toContain('Administrator access required.');
    expect(refused).toContain('data-remote-devices-state="forbidden"');
    expect(refused).not.toContain('Pairing Requests');

    const failed = render({ state: 'loading', message: 'Could not load the device list.' });
    expect(failed).toContain('role="alert"');
    expect(failed).toContain('Could not load the device list.');
  });

  it('keeps an approved request visible without offering a second approval', () => {
    const list = fixture({ local: true });
    list.pending = [{ ...list.pending[0], approved: true }];
    const markup = render({ state: 'ready', list, formatTime: at });
    expect(markup).toContain('Approved');
    expect(markup).toContain('waiting for https://h.example to finish pairing');
  });
});
