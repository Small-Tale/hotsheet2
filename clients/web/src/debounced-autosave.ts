export const AUTOSAVE_DELAY_MS = 150;

export interface DebouncedAutosave<T> {
  schedule(value: T): void;
  flush(): Promise<boolean>;
  cancel(): void;
  pending(): boolean;
}

/** Coalesces text edits while keeping blur/navigation able to flush the latest value. */
export function createDebouncedAutosave<T>(save: (value: T) => Promise<boolean>, delay = AUTOSAVE_DELAY_MS): DebouncedAutosave<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  let queued: T | undefined;
  let hasQueuedValue = false;
  let active: Promise<boolean> | undefined;

  const persist = async (): Promise<boolean> => {
    if (!hasQueuedValue) return active ?? true;
    const value = queued as T;
    queued = undefined;
    hasQueuedValue = false;
    if (timer) clearTimeout(timer);
    timer = undefined;
    active = save(value);
    try { return await active; }
    finally { active = undefined; }
  };

  return {
    schedule(value) {
      queued = value;
      hasQueuedValue = true;
      if (timer) clearTimeout(timer);
      timer = setTimeout(() => { void persist(); }, delay);
    },
    flush: persist,
    cancel() {
      if (timer) clearTimeout(timer);
      timer = undefined;
      queued = undefined;
      hasQueuedValue = false;
    },
    pending: () => hasQueuedValue || active !== undefined,
  };
}
