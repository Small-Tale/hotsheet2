import { signal } from 'kerfjs';

type Transport = { setHostHealthDisk: (id: string, monitor: boolean) => Promise<unknown> };

/**
 * Include or ignore a disk in host health (HS2-NHK8V8). The server saves the choice machine-wide and
 * pushes a `host_health` event; the caller's `refresh` re-reads the snapshot at once so the prompt
 * clears without waiting for the event. A failure keeps the prompt and explains why.
 */
export function createHostDiskChoices(transport: () => Transport, refresh: () => Promise<void>) {
  const message = signal<string | undefined>(undefined);
  let generation = 0;

  async function choose(id: string, monitor: boolean): Promise<void> {
    const request = ++generation;
    message.value = undefined;
    try {
      await transport().setHostHealthDisk(id, monitor);
      await refresh();
    } catch (reason) {
      if (request === generation) message.value = reason instanceof Error ? reason.message : String(reason);
    }
  }

  return { message, choose };
}

export type HostDiskChoices = ReturnType<typeof createHostDiskChoices>;
