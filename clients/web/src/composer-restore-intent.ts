/** Parked modality survives activation's closed placeholder, until restore or a newer user action
 * consumes it. The existing composer epoch distinguishes opening/cancel/reset from hydration
 * (HS2-40JX5Q); generations keep an interrupted activation from finishing a newer one. */
export function createComposerRestoreIntent() {
  let pending: { projectId: string; generation: number; epoch: number; open: boolean } | undefined;
  const matches = (projectId: string, generation: number) =>
    pending?.projectId === projectId && pending.generation === generation;
  return {
    capture(projectId: string, generation: number, epoch: number, open: boolean | undefined) {
      pending = open === undefined ? undefined : { projectId, generation, epoch, open };
    },
    canRestore(projectId: string, generation: number, epoch: number) {
      return !pending || (matches(projectId, generation) && pending.epoch === epoch);
    },
    persistedOpen(projectId: string, generation: number, epoch: number, open: boolean) {
      return open || (matches(projectId, generation) && pending!.epoch === epoch && pending!.open);
    },
    finish(projectId: string, generation: number) {
      if (matches(projectId, generation)) pending = undefined;
    },
  };
}
