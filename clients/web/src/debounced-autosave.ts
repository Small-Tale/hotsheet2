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
  /** Strict local-only capture for controlled native close; failures must remain observable. */
  prepareRecovery?: (value: T, inFlight?: InFlightRecovery<T>) => PreparedRecovery;
  delay?: number;
}

export interface PreparedRecovery {
  destination: string | undefined;
  capture(): void;
}

export interface InFlightRecovery<T> {
  destination: string | undefined;
  value: T;
}

export interface FocusLossAutosave<T> extends DebouncedAutosave<T> {
  captureRecovery(): void;
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
  { persist, prepareRecovery, delay = AUTOSAVE_DELAY_MS }: FocusLossAutosaveOptions<T> = {},
): FocusLossAutosave<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  let queued: T | undefined;
  let hasQueuedValue = false;
  let active: Promise<boolean> | undefined;
  // HS2-E0H7S0: independent of the server queue, including active/failed saves.
  let recovery: Partial<PreparedRecovery> | undefined;
  let inFlight: InFlightRecovery<T> | undefined;

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
    const captured = recovery;
    queued = undefined;
    hasQueuedValue = false;
    const current = save(value);
    active = current;
    const saving = { value, destination: captured?.destination };
    inFlight = saving;
    try {
      const saved = await current;
      if (saved && recovery === captured) recovery = undefined;
      return saved;
    } finally {
      if (active === current) active = undefined;
      if (inFlight === saving) inFlight = undefined;
    }
  };

  return {
    schedule(value) {
      recovery = prepareRecovery?.(value, inFlight) ?? {};
      queued = value;
      hasQueuedValue = true;
      if (timer) clearTimeout(timer);
      timer = setTimeout(persistNow, delay);
    },
    flush,
    captureRecovery() {
      if (!recovery) return;
      if (!recovery.capture) throw new Error('Local recovery capture is unavailable.');
      recovery.capture();
    },
    cancel() {
      if (timer) clearTimeout(timer);
      timer = undefined;
      queued = undefined;
      hasQueuedValue = false;
      recovery = undefined;
      inFlight = undefined; // a cancelled owner cannot supply predecessor authority to a replacement
    },
    pending: () => hasQueuedValue || active !== undefined,
  };
}

export interface CoalescedInput<T> {
  /** Records the latest draft and publishes it once `delay` passes without another keystroke. */
  schedule(value: T): void;
  /** Publishes a pending draft immediately (blur, submit, or a dependent action); no-op when idle. */
  flush(): void;
  /** Drops a pending draft without publishing it. */
  cancel(): void;
  pending(): boolean;
}

/**
 * Synchronous keystroke coalescer for controls whose app-state write re-renders a surface
 * (HS2-9CWDSB). The field keeps the live draft; app state catches up once typing pauses or when
 * the caller flushes, so the render fan-out runs once per pause instead of once per key.
 */
export function createCoalescedInput<T>(publish: (value: T) => void, delay = AUTOSAVE_DELAY_MS): CoalescedInput<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  let queued: { value: T } | undefined;
  const clear = () => {
    if (timer) clearTimeout(timer);
    timer = undefined;
  };
  const flush = () => {
    clear();
    const next = queued;
    queued = undefined;
    if (next) publish(next.value);
  };
  return {
    schedule(value) {
      queued = { value };
      clear();
      timer = setTimeout(flush, delay);
    },
    flush,
    cancel() {
      clear();
      queued = undefined;
    },
    pending: () => queued !== undefined,
  };
}

/**
 * A `createCoalescedInput` shared by several fields (HS2-2YW8R3): typing into a different field
 * (by `keyOf`) publishes the previous field's pending draft before queueing its own, so switching
 * fields mid-pause never drops text.
 */
export function createKeyedCoalescedInput<T>(
  keyOf: (value: T) => string,
  publish: (value: T) => void,
  delay = AUTOSAVE_DELAY_MS,
): CoalescedInput<T> {
  const input = createCoalescedInput(publish, delay);
  let queuedKey: string | undefined;
  return {
    ...input,
    schedule(value) {
      const key = keyOf(value);
      if (input.pending() && queuedKey !== key) input.flush();
      queuedKey = key;
      input.schedule(value);
    },
  };
}
