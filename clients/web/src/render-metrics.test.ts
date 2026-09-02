import { afterEach,describe,expect,it,vi } from 'vitest';

import { createRenderMetrics } from './render-metrics';

class FakeMutationObserver {
  static latest?: FakeMutationObserver;
  records: MutationRecord[] = [];
  constructor(callback: MutationCallback) {
    void callback;
    FakeMutationObserver.latest = this;
  }
  disconnect = vi.fn();
  observe = vi.fn();
  takeRecords() {
    return this.records.splice(0);
  }
}

describe('render metrics', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    FakeMutationObserver.latest = undefined;
  });

  it('counts render passes and pending DOM mutations and can reset its budget', () => {
    vi.stubGlobal('MutationObserver', FakeMutationObserver);
    const root = {} as Element;
    const metrics = createRenderMetrics(root);
    const observer = FakeMutationObserver.latest!;

    metrics.recordPass();
    observer.records.push({} as MutationRecord, {} as MutationRecord);
    expect(metrics.snapshot()).toEqual({ passes: 1, mutations: 2 });

    observer.records.push({} as MutationRecord);
    metrics.reset();
    expect(metrics.snapshot()).toEqual({ passes: 0, mutations: 0 });
    metrics.disconnect();
    expect(observer.observe).toHaveBeenCalledWith(root, expect.objectContaining({ subtree: true }));
    expect(observer.disconnect).toHaveBeenCalledOnce();
  });
});
