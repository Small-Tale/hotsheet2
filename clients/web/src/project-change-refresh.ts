/** Batch dense change-stream invalidations without delaying the stream's other events. */
export function createProjectChangeRefresh<Target extends { id: string }>(
  refresh: (target: Target) => Promise<unknown>,
  onError: (reason: unknown) => void,
  options: { quietMs?: number; maxWaitMs?: number; now?: () => number } = {},
) {
  const quietMs = options.quietMs ?? 180;
  const maxWaitMs = options.maxWaitMs ?? 900;
  const now = options.now ?? Date.now;
  type Entry = {
    target: Target;
    firstAt: number;
    timer?: ReturnType<typeof setTimeout>;
    running: boolean;
    dirty: boolean;
  };
  const pending = new Map<string, Entry>();
  let disposed = false;

  const arm = (id: string, entry: Entry) => {
    if (entry.timer !== undefined) clearTimeout(entry.timer);
    const remaining = Math.max(0, maxWaitMs - (now() - entry.firstAt));
    entry.timer = setTimeout(
      () => {
        entry.timer = undefined;
        if (disposed || pending.get(id) !== entry || entry.running) return;
        entry.running = true;
        entry.dirty = false;
        const target = entry.target;
        void refresh(target)
          .catch(onError)
          .finally(() => {
            entry.running = false;
            if (disposed || pending.get(id) !== entry) return;
            if (entry.dirty) arm(id, entry);
            else pending.delete(id);
          });
      },
      Math.min(quietMs, remaining),
    );
  };

  return {
    schedule(target: Target) {
      if (disposed) return;
      const entry = pending.get(target.id);
      if (entry) {
        entry.target = target;
        if (entry.running) {
          if (!entry.dirty) entry.firstAt = now();
          entry.dirty = true;
        } else arm(target.id, entry);
      } else {
        const next: Entry = { target, firstAt: now(), running: false, dirty: false };
        pending.set(target.id, next);
        arm(target.id, next);
      }
    },
    cancel(id: string) {
      const entry = pending.get(id);
      if (entry?.timer !== undefined) clearTimeout(entry.timer);
      pending.delete(id);
    },
    dispose() {
      disposed = true;
      for (const id of pending.keys()) this.cancel(id);
    },
  };
}
