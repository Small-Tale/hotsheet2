import { CatalogExample } from '@kerfjs/ui/catalog';

import { RemoteDevicesSettings } from '../components/remote-devices-settings';
import { wireRemoteDevicesSettings } from '../components/remote-devices-settings-interactions';
import { createRemoteDevices } from '../features/remote-devices';
import type { RemoteDeviceList } from '../remote-devices-model';
import type { DemoModule } from './demo-module';

/** A fixed clock so the catalog renders the same times everywhere. */
const NOW = 1_794_000_000;
const time = (seconds: number) => {
  const minutes = Math.round((NOW - seconds) / 60);
  return minutes <= 0
    ? 'now'
    : minutes < 60
      ? `${String(minutes)} min ago`
      : `${String(Math.round(minutes / 60))} h ago`;
};

function fixtureList(): RemoteDeviceList {
  return {
    server_id: '01SYNTHETICSERVER00000000',
    revision: 1,
    pair_ttl_seconds: 900,
    manager: { local: true },
    pending: [
      {
        id: '01SYNTHETICREQUEST0000001',
        label: 'Safari on iPhone',
        origin: 'https://synthetic-host.example.ts.net',
        created_at: NOW - 120,
        expires_at: NOW + 780,
        approved: false,
      },
    ],
    devices: [
      {
        id: '01SYNTHETICDEVICE00000001',
        label: 'Chrome on macOS',
        role: 'read_write',
        admin: true,
        local: true,
        origin: 'http://127.0.0.1:4175',
        created_at: NOW - 86_400,
        last_used_at: NOW,
        expires_at: NOW + 2_500_000,
      },
      {
        id: '01SYNTHETICDEVICE00000002',
        label: 'Safari on iPad, the long-named living-room tablet that reads tickets',
        role: 'read_only',
        admin: false,
        local: false,
        origin: 'https://synthetic-host.example.ts.net',
        created_at: NOW - 400_000,
        last_used_at: NOW - 7_200,
        expires_at: NOW + 2_000_000,
      },
    ],
  };
}

/**
 * The production controller over an in-memory host: changes edit the fixture and wake the pending
 * long poll, exactly as the server's revision does. No network request and no timer.
 */
function createDemoHost() {
  let list = fixtureList();
  let wake: (() => void) | undefined;
  const changed = () => {
    list = { ...list, revision: list.revision + 1 };
    wake?.();
  };
  const json = (body: unknown, status = 200) => Response.json(body, { status });
  const request: typeof fetch = async (input, init) => {
    const url = new URL(
      typeof input === 'string' ? input : input instanceof URL ? input.href : input.url,
      'http://demo',
    );
    const method = init?.method ?? 'GET';
    const body = typeof init?.body === 'string' ? (JSON.parse(init.body) as Record<string, unknown>) : {};
    if (url.pathname.endsWith('/changes')) {
      if (Number(url.searchParams.get('after')) === list.revision)
        await new Promise<void>((resolve) => {
          wake = resolve;
          init?.signal?.addEventListener(
            'abort',
            () => {
              resolve();
            },
            { once: true },
          );
        });
      return json(list);
    }
    if (method === 'GET') return json(list);
    const id = typeof body.id === 'string' ? body.id : decodeURIComponent(url.pathname.split('/').pop() ?? '');
    if (url.pathname.endsWith('/approve')) {
      const request = list.pending.find((entry) => entry.id === id);
      if (!request) return json({ error: 'pairing request not found' }, 410);
      list = {
        ...list,
        pending: list.pending.filter((entry) => entry.id !== id),
        devices: [
          ...list.devices,
          {
            id: `${id}-device`,
            label: request.label,
            role: body.role === 'read_only' ? 'read_only' : 'read_write',
            admin: false,
            local: false,
            origin: request.origin,
            created_at: NOW,
            last_used_at: NOW,
            expires_at: NOW + 2_592_000,
          },
        ],
      };
    } else if (url.pathname.endsWith('/reject')) {
      list = { ...list, pending: list.pending.filter((entry) => entry.id !== id) };
    } else if (method === 'DELETE') {
      list = { ...list, devices: list.devices.filter((entry) => entry.id !== id) };
    } else if (method === 'PATCH') {
      list = {
        ...list,
        devices: list.devices.map((entry) =>
          entry.id === id
            ? {
                ...entry,
                ...(body.role === 'read_only' || body.role === 'read_write' ? { role: body.role } : {}),
                ...(typeof body.admin === 'boolean' ? { admin: body.admin } : {}),
                ...(typeof body.local === 'boolean' ? { local: body.local } : {}),
              }
            : entry,
        ),
      };
    }
    changed();
    return new Response(null, { status: 204 });
  };
  return {
    devices: createRemoteDevices({ request, canManage: () => true }),
    reset: () => {
      list = fixtureList();
      changed();
    },
  };
}

const host = createDemoHost();

export function RemoteDevicesSettingsDemo() {
  return (
    <>
      <CatalogExample label="Interactive">
        <RemoteDevicesSettings
          state={host.devices.state.value}
          list={host.devices.list.value}
          message={host.devices.message.value}
          busy={host.devices.busy.value}
          formatTime={time}
        />
      </CatalogExample>
      <CatalogExample label="Administrator Without Host Access">
        <RemoteDevicesSettings
          state="ready"
          list={{ ...fixtureList(), manager: { device_id: '01SYNTHETICDEVICE00000002', local: false } }}
          formatTime={time}
        />
      </CatalogExample>
      <CatalogExample label="Loading">
        <RemoteDevicesSettings state="loading" />
      </CatalogExample>
      <CatalogExample label="Empty">
        <RemoteDevicesSettings state="ready" list={{ ...fixtureList(), pending: [], devices: [] }} />
      </CatalogExample>
      <CatalogExample label="Administrator Required">
        <RemoteDevicesSettings state="forbidden" />
      </CatalogExample>
      <CatalogExample label="Load Failure">
        <RemoteDevicesSettings
          state="loading"
          message="Could not load the device list. The Hot Sheet server is not responding."
        />
      </CatalogExample>
    </>
  );
}

/** The catalog entry this file renders (HS2-DXM0YN): the production controller and handlers over a fixture host. */
export const demoModule: DemoModule = {
  entries: {
    'remote-devices-settings': {
      catalogExamples: true,
      Demo: RemoteDevicesSettingsDemo,
      reset: () => {
        host.reset();
      },
    },
  },
  wire: (root) => {
    host.devices.start();
    const unwire = wireRemoteDevicesSettings(root, host.devices);
    return () => {
      unwire();
      host.devices.stop();
    };
  },
};
