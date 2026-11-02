import { createTrailingTaskInternal } from './trailing-task-internal';

export interface TrailingTask<T> {
  schedule(value: T): void;
  cancel(): void;
}

/** Collapse rapid intent changes to the last operation before expensive work starts. */
export function createTrailingTask<T>(delayMs: number, run: (value: T) => void): TrailingTask<T> {
  return createTrailingTaskInternal(delayMs, run);
}
