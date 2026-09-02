export interface RenderMetricsSnapshot {
  passes: number;
  mutations: number;
}
export interface RenderMetrics {
  recordPass(): void;
  reset(): void;
  snapshot(): RenderMetricsSnapshot;
  disconnect(): void;
}

/** Development-only counters used by browser tests to enforce narrow Kerf updates. */
export function createRenderMetrics(root: Element): RenderMetrics {
  let passes = 0;
  let mutations = 0;
  const observer = new MutationObserver(records => {
    mutations += records.length;
  });
  observer.observe(root, {
    attributes: true,
    characterData: true,
    childList: true,
    subtree: true,
  });

  return {
    recordPass() {
      passes += 1;
    },
    reset() {
      passes = 0;
      mutations = 0;
      observer.takeRecords();
    },
    snapshot() {
      mutations += observer.takeRecords().length;
      return { passes, mutations };
    },
    disconnect() {
      observer.disconnect();
    },
  };
}
