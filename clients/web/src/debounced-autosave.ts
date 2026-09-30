export const AUTOSAVE_DELAY_MS = 150;

export interface DebouncedAutosave<T> {
  schedule(value: T): void;
  flush(): Promise<boolean>;
  cancel(): void;
  pending(): boolean;
}

/** Coalesces text edits while keeping blur/navigation able to flush the latest value. */
export function createDebouncedAutosave<T>(
  save: (value: T) => Promise<boolean>,
  delay = AUTOSAVE_DELAY_MS,
): DebouncedAutosave<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  let queued: T | undefined;
  let hasQueuedValue = false;
  let active: Promise<boolean> | undefined;

  const persist = async (): Promise<boolean> => {
    if (active) {
      const saved = await active;
      return hasQueuedValue ? persist() : saved;
    }
    if (!hasQueuedValue) return true;
    const value = queued as T;
    queued = undefined;
    hasQueuedValue = false;
    if (timer) clearTimeout(timer);
    timer = undefined;
    const current = save(value);
    active = current;
    try {
      return await current;
    } finally {
      if (active === current) active = undefined;
    }
  };

  return {
    schedule(value) {
      queued = value;
      hasQueuedValue = true;
      if (timer) clearTimeout(timer);
      timer = setTimeout(() => {
        void persist();
      }, delay);
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

export interface FocusLossAutosaveOptions<T> {
  /** Keep a local recovery copy of the latest value; runs debounced while the user types. */
  persist?: (value: T) => void;
  delay?: number;
}

/**
 * Text-edit autosave that never writes to the server while the user types (HS2-RE1PS6). `schedule`
 * only records the latest value and, after `delay`, persists it locally for crash recovery; the
 * server save runs on `flush()` — when focus leaves the editing surface, the page hides, or the app
 * asks for it. Editing therefore produces one write per editing session instead of one per pause,
 * and nothing this client wrote can come back mid-edit as a stale "their" value.
 */
export function createFocusLossAutosave<T>(
  save: (value: T) => Promise<boolean>,
  { persist, delay = AUTOSAVE_DELAY_MS }: FocusLossAutosaveOptions<T> = {},
): DebouncedAutosave<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  let queued: T | undefined;
  let hasQueuedValue = false;
  let active: Promise<boolean> | undefined;

  const persistNow = () => {
    if (timer) clearTimeout(timer);
    timer = undefined;
    if (hasQueuedValue) persist?.(queued as T);
  };
  const flush = async (): Promise<boolean> => {
    if (active) {
      const saved = await active;
      return hasQueuedValue ? flush() : saved;
    }
    if (!hasQueuedValue) return true;
    persistNow();
    const value = queued as T;
    queued = undefined;
    hasQueuedValue = false;
    const current = save(value);
    active = current;
    try {
      return await current;
    } finally {
      if (active === current) active = undefined;
    }
  };

  return {
    schedule(value) {
      queued = value;
      hasQueuedValue = true;
      if (timer) clearTimeout(timer);
      timer = setTimeout(persistNow, delay);
    },
    flush,
    cancel() {
      if (timer) clearTimeout(timer);
      timer = undefined;
      queued = undefined;
      hasQueuedValue = false;
    },
    pending: () => hasQueuedValue || active !== undefined,
  };
}
