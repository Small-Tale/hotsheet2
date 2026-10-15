import { type CoalescedInput, createCoalescedInput } from './debounced-autosave';

/**
 * Keystroke coalescing for the ticket text editors (HS2-EQTXTJ). Writing an editor's draft signal
 * re-renders the whole workspace, which made every keystroke in a title, details, note, or blocked
 * reason editor wait on that render. Like HS2-9CWDSB's conflict field, the textarea keeps the live
 * text and the draft signal catches up when typing pauses. Anything that reads drafts for logic
 * (finishing or saving an edit, a refresh merge, switching tickets, page hide) calls
 * `flushDraftInputs()` first, so it always sees the text the user typed.
 */
const pending = new Set<CoalescedInput<unknown>>();

export function coalescedDraft<T>(publish: (value: T) => void, delay?: number): CoalescedInput<T> {
  const input = createCoalescedInput(publish, delay);
  const coalesced: CoalescedInput<T> = {
    schedule(value) {
      pending.add(coalesced);
      input.schedule(value);
    },
    flush() {
      pending.delete(coalesced);
      input.flush();
    },
    cancel() {
      pending.delete(coalesced);
      input.cancel();
    },
    pending: () => input.pending(),
  };
  return coalesced;
}

/** Publish every editor draft still waiting for a typing pause. */
export function flushDraftInputs(): void {
  for (const input of [...pending]) input.flush();
}
