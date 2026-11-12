import { signal } from 'kerfjs';

type Transport = {
  setHostHealthDisk: (id: string, monitor: boolean) => Promise<unknown>;
  addHostHealthDisk: (path: string) => Promise<unknown>;
  forgetHostHealthDisk: (id: string) => Promise<unknown>;
};

/** The Add Disk dialog (HS2-KBVZV1) while it is open. */
export interface AddHostDiskDialogState {
  value: string;
  message: string;
  busy: boolean;
}

const reasonText = (reason: unknown) => (reason instanceof Error ? reason.message : String(reason));

/**
 * Disk choices for host health (HS2-NHK8V8, HS2-KBVZV1): include or ignore a detected disk, add one by
 * path, or forget a saved choice. The server saves each machine-wide and pushes a `host_health` event;
 * `refresh` re-reads the snapshot at once so the UI follows without waiting for the event. A failure
 * keeps the prompt, switch, or dialog in place and explains why.
 */
export function createHostDiskChoices(transport: () => Transport, refresh: () => Promise<void>) {
  const message = signal<string | undefined>(undefined);
  const dialog = signal<AddHostDiskDialogState | undefined>(undefined);
  let generation = 0;

  /** Run one change; resolves whether it saved. Only the latest change's failure is shown. */
  async function run(change: (api: Transport) => Promise<unknown>): Promise<boolean> {
    const request = ++generation;
    message.value = undefined;
    try {
      await change(transport());
      await refresh();
      return true;
    } catch (reason) {
      if (request === generation) message.value = reasonText(reason);
      return false;
    }
  }

  /** Include or ignore a detected disk (the Stats prompt and the settings switch). */
  const choose = (id: string, monitor: boolean) => run((api) => api.setHostHealthDisk(id, monitor));
  /** Forget a saved choice (HS2-KBVZV1). */
  const forget = (id: string) => run((api) => api.forgetHostHealthDisk(id));

  /** The Add Disk dialog: open empty, keep the typed path, and close only once the disk is added. */
  const add = {
    open() {
      dialog.value = { value: '', message: '', busy: false };
    },
    input(value: string) {
      const current = dialog.peek();
      if (current) dialog.value = { ...current, value, message: '' };
    },
    cancel() {
      dialog.value = undefined;
    },
    async submit(): Promise<void> {
      const current = dialog.peek();
      if (!current || current.busy) return;
      const path = current.value.trim();
      if (!path) {
        dialog.value = { ...current, message: 'Enter a path on the disk.' };
        return;
      }
      dialog.value = { ...current, busy: true, message: '' };
      try {
        await transport().addHostHealthDisk(path);
        dialog.value = undefined;
        message.value = undefined;
        await refresh();
      } catch (reason) {
        // Closed meanwhile: nothing to report into.
        if (dialog.peek()) dialog.value = { ...current, busy: false, message: reasonText(reason) };
      }
    },
  };

  return { message, dialog, choose, forget, add };
}

export type HostDiskChoices = ReturnType<typeof createHostDiskChoices>;
