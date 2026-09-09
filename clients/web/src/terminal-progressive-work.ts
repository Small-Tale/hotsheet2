export type ProgressiveWorkSchedule = (work: () => void) => void;

export interface ProgressiveWorkQueueOptions<T> {
  mount: (item: T) => void;
  dispose: (work: () => void) => void;
  schedule: ProgressiveWorkSchedule;
  mountsPerTurn?: number;
  disposalsPerTurn?: number;
}

/** Bounds expensive terminal setup and teardown so navigation can paint between batches. */
export class ProgressiveTerminalWorkQueue<T> {
  readonly #mount: (item: T) => void;
  readonly #dispose: (work: () => void) => void;
  readonly #scheduleWork: ProgressiveWorkSchedule;
  readonly #mountsPerTurn: number;
  readonly #disposalsPerTurn: number;
  readonly #mounts: T[] = [];
  readonly #mountSet = new Set<T>();
  readonly #disposals: Array<() => void> = [];
  #scheduled = false;

  constructor(options: ProgressiveWorkQueueOptions<T>) {
    this.#mount = options.mount;
    this.#dispose = options.dispose;
    this.#scheduleWork = options.schedule;
    this.#mountsPerTurn = Math.max(1, options.mountsPerTurn ?? 2);
    this.#disposalsPerTurn = Math.max(1, options.disposalsPerTurn ?? 2);
  }

  enqueueMount(item: T) {
    if (this.#mountSet.has(item)) return;
    this.#mountSet.add(item);
    this.#mounts.push(item);
    this.#ensureScheduled();
  }

  cancelMount(item: T) {
    this.#mountSet.delete(item);
  }

  enqueueDisposal(work: () => void) {
    this.#disposals.push(work);
    this.#ensureScheduled();
  }

  get pendingMountCount() { return this.#mountSet.size; }
  get pendingDisposalCount() { return this.#disposals.length; }

  #ensureScheduled() {
    if (this.#scheduled) return;
    this.#scheduled = true;
    this.#scheduleWork(() => { this.#drain(); });
  }

  #drain() {
    this.#scheduled = false;
    let mounted = 0;
    while (this.#mounts.length > 0 && mounted < this.#mountsPerTurn) {
      const item = this.#mounts.shift()!;
      if (!this.#mountSet.delete(item)) continue;
      this.#mount(item);
      mounted += 1;
    }
    for (let index = 0; index < this.#disposalsPerTurn && this.#disposals.length > 0; index += 1) {
      this.#dispose(this.#disposals.shift()!);
    }
    if (this.#mountSet.size > 0 || this.#disposals.length > 0) this.#ensureScheduled();
  }
}
