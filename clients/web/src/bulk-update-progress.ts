/** Progress for one provider that must update selected tickets one request at a time. */
export interface BulkUpdateProgress {
  completed: number;
  total: number;
}

export interface BulkUpdateHandle {
  advance(completed: number): void;
  finish(): void;
}

/** Keep overlapping updates independent so one completion cannot hide another's indicator. */
export function createBulkUpdateProgress(publish: (progress: BulkUpdateProgress | undefined) => void) {
  const active = new Map<number, BulkUpdateProgress>();
  let nextId = 0;
  const showLatest = () => {
    publish([...active.values()].at(-1));
  };
  return {
    begin: (total: number): BulkUpdateHandle => {
      const id = ++nextId;
      active.set(id, { completed: 0, total });
      showLatest();
      return {
        advance(completed) {
          const current = active.get(id);
          if (!current) return;
          active.set(id, { ...current, completed: Math.max(0, Math.min(completed, current.total)) });
          showLatest();
        },
        finish() {
          if (!active.delete(id)) return;
          showLatest();
        },
      };
    },
  };
}
