import { describe, expect, it, vi } from 'vitest';

import { createHostCpuFeed, type HostCpuFrame, parseHostCpuFrame } from './host-cpu-feed';
import { ServerResponseShapeError } from './server-response-parsers';

const frame = (seq: number, status: HostCpuFrame['status'] = 'live'): HostCpuFrame => ({
  seq,
  status,
  sampled_at: status === 'live' ? '2026-11-12T10:00:00Z' : null,
  busy_percent: status === 'live' ? 40 : null,
  core_percent: status === 'live' ? [80, 0] : [],
  interval_ms: 2000,
});

/** A scripted server: each request waits until the test answers it. */
function server() {
  const requests: {
    after: number | undefined;
    resolve: (value: HostCpuFrame) => void;
    reject: (reason: unknown) => void;
    signal: AbortSignal;
  }[] = [];
  const hostCpu = vi.fn(
    (after: number | undefined, signal: AbortSignal) =>
      new Promise<HostCpuFrame>((resolve, reject) => {
        requests.push({ after, resolve, reject, signal });
      }),
  );
  return { requests, transport: () => ({ hostCpu }), hostCpu };
}

const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

describe('HS2-CPFBWP live CPU feed', () => {
  it('parses frames and rejects drift', () => {
    expect(parseHostCpuFrame(frame(3))).toEqual(frame(3));
    expect(parseHostCpuFrame(frame(0, 'unsupported')).status).toBe('unsupported');
    for (const drifted of [
      null,
      { ...frame(1), status: 'busy' },
      { ...frame(1), seq: -1 },
      { ...frame(1), core_percent: ['80'] },
      { ...frame(1), busy_percent: '40' },
      { ...frame(1), interval_ms: undefined },
    ])
      expect(() => parseHostCpuFrame(drifted)).toThrow(ServerResponseShapeError);
  });

  it('long-polls with the held cursor, paced only by answers, and stops cleanly', async () => {
    const { requests, transport, hostCpu } = server();
    const feed = createHostCpuFeed(transport);
    expect(hostCpu).not.toHaveBeenCalled();
    feed.start();
    feed.start(); // a second start is a no-op
    await settle();
    expect(requests.map((request) => request.after)).toEqual([undefined]);
    requests[0]?.resolve(frame(7));
    await settle();
    expect(feed.state.value.frame?.seq).toBe(7);
    expect(requests.map((request) => request.after)).toEqual([undefined, 7]);
    // Nothing else is requested until the server answers.
    await settle();
    expect(hostCpu).toHaveBeenCalledTimes(2);
    requests[1]?.resolve(frame(8));
    await settle();
    expect(requests.at(-1)?.after).toBe(8);
    feed.stop();
    expect(requests.at(-1)?.signal.aborted).toBe(true);
    expect(feed.running()).toBe(false);
    await settle();
    expect(hostCpu).toHaveBeenCalledTimes(3);
    // A restart resumes from the frame it holds.
    feed.start();
    await settle();
    expect(requests.at(-1)?.after).toBe(8);
    feed.stop();
  });

  it('backs off on failures and unavailable frames, resets after a good frame, and ends on unsupported', async () => {
    const { requests, transport } = server();
    const waits: number[] = [];
    const feed = createHostCpuFeed(transport, {
      retryMs: 1000,
      maxRetryMs: 4000,
      wait: (ms) => {
        waits.push(ms);
        return Promise.resolve();
      },
    });
    feed.start();
    await settle();
    requests[0]?.reject(new Error('offline'));
    await settle();
    expect(feed.state.value.error).toBe('offline');
    requests[1]?.reject(new Error('offline'));
    await settle();
    requests[2]?.reject(new Error('offline'));
    await settle();
    requests[3]?.resolve(frame(1, 'unavailable'));
    await settle();
    expect(waits).toEqual([1000, 2000, 4000, 1000]);
    expect(feed.state.value).toEqual({ frame: frame(1, 'unavailable') });
    requests[4]?.resolve(frame(2));
    await settle();
    requests[5]?.resolve(frame(3, 'unavailable'));
    await settle();
    expect(waits.at(-1)).toBe(1000);
    requests[6]?.resolve(frame(0, 'unsupported'));
    await settle();
    expect(requests).toHaveLength(7);
    expect(feed.state.value.frame?.status).toBe('unsupported');
  });
});
