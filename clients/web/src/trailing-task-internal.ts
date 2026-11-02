// HS2-K5E2KF: timer mechanics stay behind the trailing-task public module.
export function createTrailingTaskInternal<T>(delayMs: number, run: (value: T) => void) {
  let timer: ReturnType<typeof setTimeout> | undefined;
  return {
    schedule(value: T) {
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
