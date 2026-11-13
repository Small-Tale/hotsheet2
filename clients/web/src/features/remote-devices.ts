import { signal } from 'kerfjs';

import { canManageRemoteDevices } from '../client-origin';
import { bridgeErrorMessage } from '../local-bridge-client';
import {
  parseRemoteDeviceList,
  type RemoteDeviceChange,
  type RemoteDeviceList,
  type RemoteDeviceRole,
} from '../remote-devices-model';

/** The bridge paths App Settings → Devices uses (HS2-DXM0YN). */
export const REMOTE_DEVICES_PATH = '/__hotsheet/remote-auth/owner';

/** Where the Devices category stands: not shown, loading, showing a list, or refused. */
export type RemoteDevicesState = 'idle' | 'loading' | 'ready' | 'forbidden';

/** Failure backoff for the device-list long poll: 1 s doubling to 30 s, reset by any answer. */
export const REMOTE_DEVICES_BACKOFF = { initialMs: 1_000, maxMs: 30_000 } as const;

/** Wait `ms` unless `signal` aborts first. */
function pause(ms: number, signal: AbortSignal): Promise<void> {
  return new Promise((resolve) => {
    const timer = setTimeout(resolve, ms);
    signal.addEventListener(
      'abort',
      () => {
        clearTimeout(timer);
        resolve();
      },
      { once: true },
    );
  });
}

const messageOf = (reason: unknown) => (reason instanceof Error ? reason.message : String(reason));

/**
 * App Settings → Devices (HS2-DXM0YN): the host's paired and pending browser devices. While the
 * category shows, one genuine long poll (`/remote-auth/owner/changes?after=<revision>`) keeps the list
 * current; a failure retries with bounded backoff, and nothing polls on a fixed interval. A 401 or
 * 403 means this session lacks the admin grant, which the category explains instead of retrying.
 */
export function createRemoteDevices(
  dependencies: {
    request?: typeof fetch;
    canManage?: () => boolean;
    wait?: (ms: number, signal: AbortSignal) => Promise<void>;
  } = {},
) {
  const request = dependencies.request ?? ((input, init) => fetch(input, init)),
    canManage = dependencies.canManage ?? (() => canManageRemoteDevices()),
    wait = dependencies.wait ?? pause;
  const list = signal<RemoteDeviceList | undefined>(undefined),
    state = signal<RemoteDevicesState>('idle'),
    message = signal(''),
    busy = signal<readonly string[]>([]);
  let running: AbortController | undefined;
  // A plain mirror of `list`: `start` runs inside the runtime's category effect, which must not
  // subscribe to every list update.
  let current: RemoteDeviceList | undefined;

  async function read(path: string, abort: AbortSignal): Promise<'ok' | 'forbidden' | 'failed' | 'aborted'> {
    try {
      const response = await request(path, { credentials: 'same-origin', signal: abort });
      if (response.status === 401 || response.status === 403) return 'forbidden';
      const body: unknown = await response.json().catch(() => undefined);
      if (!response.ok) throw new Error(bridgeErrorMessage(body) ?? 'Could not load the device list.');
      const next = parseRemoteDeviceList(body);
      if (abort.aborted) return 'aborted';
      current = next;
      list.value = next;
      state.value = 'ready';
      if (message.value.startsWith('Could not load')) message.value = '';
      return 'ok';
    } catch (reason) {
      if (abort.aborted) return 'aborted';
      message.value = `Could not load the device list. ${messageOf(reason)}`.trim();
      return 'failed';
    }
  }

  async function follow(abort: AbortSignal) {
    let backoff: number = REMOTE_DEVICES_BACKOFF.initialMs;
    while (!abort.aborted) {
      const outcome = await read(
        current ? `${REMOTE_DEVICES_PATH}/changes?after=${String(current.revision)}` : REMOTE_DEVICES_PATH,
        abort,
      );
      if (outcome === 'aborted') return;
      if (outcome === 'forbidden') {
        current = undefined;
        list.value = undefined;
        state.value = 'forbidden';
        return;
      }
      if (outcome === 'ok') backoff = REMOTE_DEVICES_BACKOFF.initialMs;
      else {
        await wait(backoff, abort);
        backoff = Math.min(backoff * 2, REMOTE_DEVICES_BACKOFF.maxMs);
      }
    }
  }

  /** Begin following the list when the category shows; a second call keeps the running poll. */
  function start() {
    if (running) return;
    if (!canManage()) {
      state.value = 'forbidden';
      return;
    }
    running = new AbortController();
    if (!current) state.value = 'loading';
    void follow(running.signal);
  }

  /** Stop following when the category hides. The last list stays for the next opening. */
  function stop() {
    running?.abort();
    running = undefined;
    // Written, never read: this runs inside the runtime's category effect.
    state.value = current ? 'ready' : 'idle';
  }

  /** Run one change; the long poll then reports the server's new list. */
  async function change(id: string, method: 'POST' | 'PATCH' | 'DELETE', path: string, body?: unknown) {
    if (busy.value.includes(id)) return false;
    busy.value = [...busy.value, id];
    message.value = '';
    try {
      const response = await request(path, {
        method,
        credentials: 'same-origin',
        ...(body === undefined ? {} : { headers: { 'content-type': 'application/json' }, body: JSON.stringify(body) }),
      });
      if (!response.ok) {
        const answer: unknown = await response.json().catch(() => undefined);
        throw new Error(bridgeErrorMessage(answer) ?? 'The host refused the change.');
      }
      return true;
    } catch (reason) {
      message.value = messageOf(reason);
      return false;
    } finally {
      busy.value = busy.value.filter((item) => item !== id);
    }
  }

  return {
    list,
    state,
    message,
    busy,
    start,
    stop,
    approve: (id: string, role: RemoteDeviceRole, grants: { admin?: boolean; local?: boolean } = {}) =>
      change(id, 'POST', `${REMOTE_DEVICES_PATH}/approve`, { id, role, ...grants }),
    reject: (id: string) => change(id, 'POST', `${REMOTE_DEVICES_PATH}/reject`, { id }),
    update: (id: string, update: RemoteDeviceChange) =>
      change(id, 'PATCH', `${REMOTE_DEVICES_PATH}/devices/${encodeURIComponent(id)}`, update),
    revoke: (id: string) => change(id, 'DELETE', `${REMOTE_DEVICES_PATH}/devices/${encodeURIComponent(id)}`),
  };
}

export type RemoteDevices = ReturnType<typeof createRemoteDevices>;
