export interface TrailingTask<T> {
  schedule(value: T): void;
  cancel(): void;
}

/** Collapse rapid intent changes to the last operation before expensive work starts. */
export function createTrailingTask<T>(delayMs: number, run: (value: T) => void): TrailingTask<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  return {
    schedule(value) {
      if (timer !== undefined) clearTimeout(timer);
      timer = setTimeout(() => {
        timer = undefined;
        run(value);
      }, delayMs);
    },
    cancel() {
      if (timer !== undefined) clearTimeout(timer);
      timer = undefined;
    },
  };
}
