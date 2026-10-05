import { describe, expect, it, type Mock, vi } from 'vitest';

import { createToastPresentation } from './toast-presentation';

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((complete) => {
    resolve = complete;
  });
  return { promise, resolve };
}

function fixture() {
  type Item = { isConnected: boolean; hide: Mock<() => Promise<void>>; remove: Mock<() => void> };
  const requests: {
      message: string;
      item: Item;
      ready: ReturnType<typeof deferred<Item>>;
    }[] = [],
    prepare = vi.fn(),
    clear = vi.fn(() => {
      for (const { item } of requests) item.isConnected = false;
    }),
    presentation = createToastPresentation({
      create: (message) => {
        const item = {
            isConnected: true,
            hide: vi.fn<() => Promise<void>>().mockResolvedValue(undefined),
            remove: vi.fn<() => void>(),
          },
          ready = deferred<typeof item>();
        requests.push({ message, item, ready });
        return ready.promise;
      },
      clear,
      prepare,
    });
  return { requests, prepare, clear, presentation };
}

describe('asynchronous toast presentation', () => {
  it('prepares only after the public factory completes and acknowledges manual dismissal once', async () => {
    const { presentation, requests, prepare } = fixture(),
      pending = presentation.publish({ message: 'Copied', generation: 1 });
    expect(requests[0].message).toBe('Copied');
    expect(prepare).not.toHaveBeenCalled();
    requests[0].ready.resolve(requests[0].item);
    await pending;
    expect(prepare).toHaveBeenCalledWith(requests[0].item, 1);
    expect(presentation.dismissed(requests[0].item)).toBe(1);
    expect(presentation.dismissed(requests[0].item)).toBeUndefined();
  });

  it('rejects out-of-order replacement results even when their messages match', async () => {
    const { presentation, requests, prepare } = fixture(),
      first = presentation.publish({ message: 'Copied', generation: 1 }),
      second = presentation.publish({ message: 'Copied', generation: 2 });
    requests[1].ready.resolve(requests[1].item);
    await second;
    requests[0].ready.resolve(requests[0].item);
    await first;
    expect(prepare.mock.calls).toEqual([[requests[1].item, 2]]);
    expect(requests[0].item.remove).toHaveBeenCalledOnce();
    expect(presentation.dismissed(requests[0].item)).toBeUndefined();
    expect(presentation.dismissed(requests[1].item)).toBe(2);
  });

  it('removes a late factory result after its application lifetime expires', async () => {
    const { presentation, requests, prepare } = fixture(),
      pending = presentation.publish({ message: 'Copied', generation: 1 });
    await presentation.publish({ message: '', generation: 1 });
    requests[0].ready.resolve(requests[0].item);
    await pending;
    expect(prepare).not.toHaveBeenCalled();
    expect(requests[0].item.remove).toHaveBeenCalledOnce();
  });

  it('does not let an old hide completion dismiss a replacement', async () => {
    const { presentation, requests } = fixture(),
      first = presentation.publish({ message: 'First', generation: 1 });
    requests[0].ready.resolve(requests[0].item);
    await first;
    const hidden = deferred<undefined>();
    requests[0].item.hide.mockImplementation(() => hidden.promise);
    const hiding = presentation.publish({ message: '', generation: 1 }),
      second = presentation.publish({ message: 'Second', generation: 2 });
    requests[1].ready.resolve(requests[1].item);
    await second;
    hidden.resolve(undefined);
    await hiding;
    expect(presentation.dismissed(requests[0].item)).toBeUndefined();
    expect(presentation.dismissed(requests[1].item)).toBe(2);
  });

  it('disposes pending work and ignores subsequent publications', async () => {
    const { presentation, requests, prepare, clear } = fixture(),
      pending = presentation.publish({ message: 'Copied', generation: 1 });
    presentation.dispose();
    requests[0].ready.resolve(requests[0].item);
    await pending;
    await presentation.publish({ message: 'Later', generation: 2 });
    expect(clear).toHaveBeenCalledTimes(2);
    expect(requests).toHaveLength(1);
    expect(prepare).not.toHaveBeenCalled();
    expect(requests[0].item.remove).toHaveBeenCalledOnce();
  });

  it('does not adopt a factory item already removed by its own lifecycle', async () => {
    const { presentation, requests, prepare } = fixture(),
      pending = presentation.publish({ message: 'Copied', generation: 1 });
    requests[0].item.isConnected = false;
    requests[0].ready.resolve(requests[0].item);
    await pending;
    expect(prepare).not.toHaveBeenCalled();
    expect(presentation.dismissed(requests[0].item)).toBeUndefined();
  });
});
