/**
 * HS2-R1KHPN: one long-lived native directory watcher shared by every long poll that
 * observes a file in that directory.
 *
 * On macOS libuv keeps a single FSEvents stream per process and re-registers it with
 * `fseventsd` whenever a watcher opens or closes; closing waits for that on the JS
 * thread. A watcher per long poll therefore stalled the event loop at every poll
 * boundary. The shared watcher is reference-counted and closes only after its
 * directory has gone unwatched for a linger window.
 *
 * FSEvents also registers asynchronously and drops earlier changes. As in the Rust
 * store watcher (HS2-P702Y8), each registration writes probe files with bounded
 * backoff until the stream delivers an event; until then every unanswered round, and
 * the proof itself, asks subscribers for a catch-up read (HS2-BN9MMJ). A subscriber
 * reads once after subscribing, so a change is either in that read, in a catch-up
 * read, or delivered by the proven stream.
 */

/** The subset of a native directory watcher this module relies on. */
export interface DirectoryWatcher {
  close(): void;
  on(event: 'error', listener: (error: Error) => void): unknown;
}
/** Starts a native watcher; it may throw (or later emit `error`) when the OS refuses one. */
export type WatchDirectory = (directory: string, changed: (filename: string | null) => void) => DirectoryWatcher;

export interface DirectorySubscriber {
  /** The stream reported a change to the subscribed file. */
  changed: () => void;
  /** Read the subscribed file again: the stream may have missed a change. */
  recheck: () => void;
  /** The native watcher failed; the subscription has ended and the caller must degrade. */
  failed: () => void;
}

export interface SharedDirectoryWatchOptions {
  directory: string;
  watchDirectory: WatchDirectory;
  /** Writes an empty probe file with this name into the watched directory. */
  writeProbe: (name: string) => Promise<void>;
  removeProbe: (name: string) => Promise<void>;
  /** How long an unwatched directory keeps its native watcher. */
  lingerMs?: number;
  /** The first probe round; each unanswered round doubles it up to `probeWaitMaxMs`. */
  probeWaitMs?: number;
  probeWaitMaxMs?: number;
}

/** Probe file names; a dot-file ending in `.tmp` is never a migration checkpoint. */
export const DIRECTORY_PROBE_PREFIX = '.hotsheet-watch-probe.';
export const SHARED_WATCH_LINGER_MS = 60_000;
export const SHARED_WATCH_PROBE_WAIT_MS = 100;
export const SHARED_WATCH_PROBE_WAIT_MAX_MS = 5_000;
let probeSequence = 0;

/** Never keep a process alive for a lingering watcher or an unanswered probe. */
function unrefTimer(handle: ReturnType<typeof setTimeout>) {
  (handle as unknown as { unref?: () => void }).unref?.();
  return handle;
}

export class SharedDirectoryWatch {
  private native?: DirectoryWatcher;
  /** Invalidates callbacks from a watcher that has since been closed or replaced. */
  private generation = 0;
  private proven = false;
  private subscribers = new Map<number, { filename: string; subscriber: DirectorySubscriber }>();
  private nextSubscriber = 0;
  private linger?: ReturnType<typeof setTimeout>;
  private probeTimer?: ReturnType<typeof setTimeout>;
  private probeWait = 0;
  private probes = new Set<string>();

  constructor(private options: SharedDirectoryWatchOptions) {}

  /** Whether a native watcher is currently open (for tests and diagnostics). */
  get open(): boolean {
    return this.native !== undefined;
  }

  /** Whether the open watcher has delivered an event since it registered. */
  get live(): boolean {
    return this.native !== undefined && this.proven;
  }

  get subscriberCount(): number {
    return this.subscribers.size;
  }

  /**
   * Observe `filename` in the directory. When the OS refuses a watcher, `failed` runs
   * synchronously and the returned unsubscribe is a no-op.
   */
  subscribe(filename: string, subscriber: DirectorySubscriber): () => void {
    clearTimeout(this.linger);
    this.linger = undefined;
    const id = this.nextSubscriber++;
    this.subscribers.set(id, { filename, subscriber });
    if (!this.native) this.start();
    else if (!this.proven && !this.probeTimer) this.startProbing();
    return () => {
      if (!this.subscribers.delete(id) || this.subscribers.size) return;
      this.stopProbing();
      if (this.native)
        this.linger = unrefTimer(
          setTimeout(() => {
            this.linger = undefined;
            this.teardown();
          }, this.options.lingerMs ?? SHARED_WATCH_LINGER_MS),
        );
    };
  }

  /** Close the native watcher now and end every subscription without notifying it. */
  close(): void {
    clearTimeout(this.linger);
    this.linger = undefined;
    this.subscribers.clear();
    this.teardown();
  }

  private start() {
    const generation = ++this.generation;
    this.proven = false;
    try {
      const native = this.options.watchDirectory(this.options.directory, (filename) => {
        if (generation === this.generation) this.delivered(filename);
      });
      this.native = native;
      native.on('error', () => {
        if (generation === this.generation) this.fail();
      });
    } catch {
      this.fail();
      return;
    }
    this.startProbing();
  }

  private delivered(filename: string | null) {
    if (!this.proven) {
      // FSEvents and inotify deliver in order: once any event arrives, every later
      // change is reported, so one catch-up read covers what registration dropped.
      this.proven = true;
      this.stopProbing();
      this.notify((entry) => {
        entry.subscriber.recheck();
      });
    }
    if (filename === null)
      this.notify((entry) => {
        entry.subscriber.recheck();
      });
    else if (!filename.startsWith(DIRECTORY_PROBE_PREFIX))
      this.notify((entry) => {
        if (entry.filename === filename) entry.subscriber.changed();
      });
  }

  /** Runs `each` over a snapshot: a subscriber may unsubscribe from its callback. */
  private notify(each: (entry: { filename: string; subscriber: DirectorySubscriber }) => void) {
    for (const entry of [...this.subscribers.values()]) each(entry);
  }

  private startProbing() {
    this.probeWait = this.options.probeWaitMs ?? SHARED_WATCH_PROBE_WAIT_MS;
    this.probe();
  }

  private probe() {
    if (this.proven || !this.native || !this.subscribers.size) return;
    const name = `${DIRECTORY_PROBE_PREFIX}${process.pid}.${probeSequence++}.tmp`;
    this.probes.add(name);
    // An unwritable directory leaves only the catch-up rounds; they still bridge.
    this.options.writeProbe(name).catch(() => {});
    this.probeTimer = unrefTimer(
      setTimeout(() => {
        this.probeTimer = undefined;
        this.removeProbes();
        this.notify((entry) => {
          entry.subscriber.recheck();
        });
        this.probeWait = Math.min(this.probeWait * 2, this.options.probeWaitMaxMs ?? SHARED_WATCH_PROBE_WAIT_MAX_MS);
        this.probe();
      }, this.probeWait),
    );
  }

  private stopProbing() {
    clearTimeout(this.probeTimer);
    this.probeTimer = undefined;
    this.removeProbes();
  }

  private removeProbes() {
    for (const name of this.probes) this.options.removeProbe(name).catch(() => {});
    this.probes.clear();
  }

  private fail() {
    const failed = [...this.subscribers.values()];
    this.subscribers.clear();
    clearTimeout(this.linger);
    this.linger = undefined;
    this.teardown();
    for (const entry of failed) entry.subscriber.failed();
  }

  private teardown() {
    this.generation++;
    this.proven = false;
    this.stopProbing();
    const native = this.native;
    this.native = undefined;
    native?.close();
  }
}
