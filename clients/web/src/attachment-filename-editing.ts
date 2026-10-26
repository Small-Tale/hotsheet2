import type { Signal } from 'kerfjs';

import { createFocusLossAutosave } from './debounced-autosave';
import { clearTicketDraft, loadTicketDraft, saveTicketDraft } from './ticket-draft-store';
import { reconcileActiveDraft } from './ticket-field-reconciliation';

export interface AttachmentFilenameEditing {
  scope: string;
  ticketKey: string;
  id: string;
  original: string;
  draft: string;
  saving: boolean;
  message: string;
}

/** A filename edit owns its captured destination, never the current sidebar selection. */
export function createAttachmentFilenameEditor(
  editing: Signal<AttachmentFilenameEditing | undefined>,
  storage: () => Storage | undefined = () => (typeof localStorage === 'undefined' ? undefined : localStorage),
) {
  let destination: { key: string; save: (filename: string) => Promise<void> } | undefined;
  const autosave = createFocusLossAutosave(
    async (edit: AttachmentFilenameEditing) => {
      const target = destination;
      if (!target) return false;
      const name = edit.draft.trim();
      if (!name || name === edit.original) {
        const store = storage();
        if (store) clearTicketDraft(store, target.key);
        if (editing.value === edit) editing.value = undefined;
        return true;
      }
      editing.value = { ...edit, saving: true, message: '' };
      try {
        await target.save(name);
        const store = storage();
        if (store) clearTicketDraft(store, target.key);
        editing.value = undefined;
        return true;
      } catch (reason) {
        editing.value = { ...edit, saving: false, message: reason instanceof Error ? reason.message : String(reason) };
        return false;
      }
    },
    {
      persist(edit) {
        const store = storage();
        if (store && destination) saveTicketDraft(store, destination.key, { base: edit.original, draft: edit.draft });
      },
    },
  );
  return {
    begin(
      edit: Pick<AttachmentFilenameEditing, 'scope' | 'ticketKey' | 'id' | 'original'>,
      key: string,
      save: (filename: string) => Promise<void>,
    ) {
      if (editing.value?.saving) return false;
      const previous = editing.value,
        previousStore = storage();
      if (previous && previousStore && destination)
        saveTicketDraft(previousStore, destination.key, { base: previous.original, draft: previous.draft });
      autosave.cancel();
      destination = { key, save };
      const store = storage(),
        copy = store && loadTicketDraft(store, key);
      const restored = copy ? reconcileActiveDraft(copy.base, copy.draft, edit.original) : undefined;
      const draft = restored?.draft ?? edit.original;
      const message =
        restored?.kind === 'conflict'
          ? 'An unsaved filename edit conflicts with a newer filename. Review it before finishing.'
          : '';
      editing.value = { ...edit, draft, saving: false, message };
      autosave.schedule(editing.value);
      return true;
    },
    input(draft: string) {
      const current = editing.value;
      if (!current || current.saving) return;
      editing.value = { ...current, draft, message: '' };
      autosave.schedule(editing.value);
    },
    cancel() {
      if (editing.value?.saving) return;
      autosave.cancel();
      const store = storage();
      if (store && destination) clearTicketDraft(store, destination.key);
      editing.value = undefined;
      destination = undefined;
    },
    finish() {
      // A failed write has no queued value: explicitly retry the preserved draft on the next finish.
      if (editing.value && !editing.value.saving) autosave.schedule(editing.value);
      return autosave.flush();
    },
    dispose() {
      autosave.cancel();
    },
  };
}
