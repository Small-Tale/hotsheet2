import { describe, expect, it, vi } from 'vitest';

import type { ChangeEvent } from './api';
import type { HostHealthSnapshot } from './host-health';
import { createHostHealthResource } from './host-health-resource';

const snapshot = (level: HostHealthSnapshot['level']) => ({ level }) as HostHealthSnapshot;
const event = (cursor: number | undefined, kind = 'host_health', id = 'disk'): ChangeEvent => ({
  cursor,
  store: '',
  kind,
  id,
  slug: 'warning',
});

/** A transport whose reads resolve only when the test says so (no timers). */
function gatedTransport() {
  const pending: Array<{ resolve: (value: HostHealthSnapshot) => void; reject: (reason: Error) => void }> = [];
  const hostHealth = vi.fn(
    () =>
      new Promise<HostHealthSnapshot>((resolve, reject) => {
        pending.push({ resolve, reject });
      }),
  );
  const next = () => {
    const read = pending.shift();
    if (!read) throw new Error('no pending read');
    return read;
  };
  return { hostHealth, next };
}

const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

describe('host health resource (HS2-DTAVQA)', () => {
  it('reads once on resync and again only for new host_health events', async () => {
    const transport = gatedTransport();
    const resource = createHostHealthResource(() => transport);
    const first = resource.resync();
    expect(resource.state.value.loading).toBe(true);
    transport.next().resolve(snapshot('normal'));
    await first;
    expect(resource.state.value).toEqual({ snapshot: snapshot('normal'), loading: false });

    // Unrelated kinds never read.
    expect(resource.applyEvents([event(4, 'created')])).toBeUndefined();
    expect(transport.hostHealth).toHaveBeenCalledTimes(1);

    const second = resource.applyEvents([event(5)]);
    expect(second).toBeDefined();
    transport.next().resolve(snapshot('warning'));
    await second;
    expect(resource.state.value.snapshot?.level).toBe('warning');

    // The same cursor through another project stream of this server is not new.
    expect(resource.applyEvents([event(5)])).toBeUndefined();
    expect(resource.applyEvents([event(3)])).toBeUndefined();
    expect(transport.hostHealth).toHaveBeenCalledTimes(2);
  });

  it('coalesces a burst into one follow-up read', async () => {
    const transport = gatedTransport();
    const resource = createHostHealthResource(() => transport);
    const read = resource.applyEvents([event(1)]);
    void resource.applyEvents([event(2)]);
    void resource.applyEvents([event(3, 'host_health', 'cpu')]);
    expect(transport.hostHealth).toHaveBeenCalledTimes(1);
    transport.next().resolve(snapshot('normal'));
    await settle();
    expect(transport.hostHealth).toHaveBeenCalledTimes(2);
    transport.next().resolve(snapshot('critical'));
    await read;
    expect(transport.hostHealth).toHaveBeenCalledTimes(2);
    expect(resource.state.value.snapshot?.level).toBe('critical');
  });

  it('keeps the last snapshot on a failed read and recovers on the next event', async () => {
    const transport = gatedTransport();
    const resource = createHostHealthResource(() => transport);
    const first = resource.refresh();
    transport.next().resolve(snapshot('warning'));
    await first;
    const failed = resource.applyEvents([event(1)]);
    transport.next().reject(new Error('Not Found'));
    await failed;
    expect(resource.state.value).toEqual({ snapshot: snapshot('warning'), loading: false, error: 'Not Found' });
    const recovered = resource.applyEvents([event(2)]);
    transport.next().resolve(snapshot('normal'));
    await recovered;
    expect(resource.state.value).toEqual({ snapshot: snapshot('normal'), loading: false });
  });

  it('accepts a restarted server whose cursors start over after resync', async () => {
    const transport = gatedTransport();
    const resource = createHostHealthResource(() => transport);
    const first = resource.applyEvents([event(900)]);
    transport.next().resolve(snapshot('normal'));
    await first;
    expect(resource.applyEvents([event(2)])).toBeUndefined();
    const resynced = resource.resync();
    transport.next().resolve(snapshot('normal'));
    await resynced;
    const restarted = resource.applyEvents([event(2)]);
    expect(restarted).toBeDefined();
    transport.next().resolve(snapshot('critical'));
    await restarted;
    expect(resource.state.value.snapshot?.level).toBe('critical');
  });

  it('ignores reads that finish after dispose', async () => {
    const transport = gatedTransport();
    const resource = createHostHealthResource(() => transport);
    const read = resource.refresh();
    resource.dispose();
    transport.next().resolve(snapshot('critical'));
    await read;
    expect(resource.state.value.snapshot).toBeUndefined();
    await resource.refresh();
    expect(transport.hostHealth).toHaveBeenCalledTimes(1);
  });
});
