import { signal } from 'kerfjs';

import {
  deleteDraftFiles,
  loadDraftFiles,
  saveDraftFile,
  type StoredDraftFile,
  type StoredPendingAttachment,
} from './workspace-session';

export interface DraftFileStore {
  save: (scope: string, id: string, file: File) => Promise<void>;
  load: (
    scope: string,
    items: readonly StoredPendingAttachment[],
  ) => Promise<{ files: StoredDraftFile[]; failed: StoredPendingAttachment[] }>;
  remove: (scope: string, ids: readonly string[]) => Promise<void>;
}

/**
 * Staged draft attachments whose recovery copy could not be written or read (HS2-791W19).
 * A failed save keeps the File in memory, still attached, and retries on the next change; a
 * failed load keeps the reference so a later session save cannot drop a file that still exists.
 */
export function createDraftFileRecovery(
  store: DraftFileStore = { save: saveDraftFile, load: loadDraftFiles, remove: deleteDraftFiles },
) {
  const unsaved = new Map<string, { scope: string; id: string; file: File; failed: boolean }>(),
    key = (scope: string, id: string) => `${scope}\u0000${id}`;
  const revision = signal(0);
  let retrying: Promise<void> | undefined;
  async function save(scope: string, id: string, file: File) {
    const entry = { scope, id, file, failed: unsaved.get(key(scope, id))?.failed ?? false };
    unsaved.set(key(scope, id), entry);
    revision.value += 1;
    try {
      await store.save(scope, id, file);
    } catch (reason) {
      if (unsaved.get(key(scope, id)) === entry) {
        entry.failed = true;
        revision.value += 1;
      }
      throw reason;
    }
    // A removal while this save was in flight already forgot it; keep that decision.
    if (unsaved.get(key(scope, id)) === entry) {
      unsaved.delete(key(scope, id));
      revision.value += 1;
    }
  }
  return {
    /** Derived from all current references, never from a previous warning string (HS2-791W19). */
    state(scope: string, files: readonly StoredPendingAttachment[], unrestored: readonly StoredPendingAttachment[]) {
      // Reading the revision lets real parent compositions react to background retry completion.
      void revision.value;
      const pending = files.filter((item) => unsaved.has(key(scope, item.id))),
        failed = pending.filter((item) => unsaved.get(key(scope, item.id))?.failed);
      return {
        attachments: [...files, ...unrestored],
        blocked: pending.length > 0 || unrestored.length > 0,
        message: [
          failed.length ? unsavedDraftFilesMessage(failed.map((item) => item.name)) : '',
          unrestored.length
            ? `${unrestoredDraftFilesMessage(unrestored.length)} Affected: ${unrestored.map((item) => item.name).join(', ')}.`
            : '',
        ]
          .filter(Boolean)
          .join(' '),
      };
    },
    /** Save each staged file; resolves to the ids whose recovery copy is still unsaved. */
    async stage(scope: string, items: readonly { id: string; file: File }[]): Promise<string[]> {
      const results = await Promise.allSettled(items.map((item) => save(scope, item.id, item.file)));
      return items.filter((_, index) => results[index]?.status === 'rejected').map((item) => item.id);
    },
    /** Retry every unsaved recovery copy once; resolves when no retry is in flight. */
    retry(): Promise<void> {
      if (retrying) return retrying;
      if (!unsaved.size) return Promise.resolve();
      retrying = (async () => {
        for (const entry of [...unsaved.values()]) {
          if (unsaved.get(key(entry.scope, entry.id)) !== entry) continue;
          await save(entry.scope, entry.id, entry.file).catch(() => undefined);
        }
      })().finally(() => {
        retrying = undefined;
      });
      return retrying;
    },
    /** How many of `ids` (or of the whole scope) still lack a recovery copy. */
    unsavedCount(scope: string, ids?: readonly string[]) {
      return [...unsaved.values()].filter((entry) => entry.scope === scope && (!ids || ids.includes(entry.id))).length;
    },
    /**
     * Restore staged files: an unsaved file still in memory wins. Missing and unreadable copies
     * stay referenced until recovery or explicit removal, so attachment intent is never lost.
     */
    async restore(scope: string, items: readonly StoredPendingAttachment[]) {
      const inMemory = items.flatMap((item) => {
          const entry = unsaved.get(key(scope, item.id));
          return entry ? [{ id: item.id, name: entry.file.name, file: entry.file }] : [];
        }),
        rest = items.filter((item) => !unsaved.has(key(scope, item.id)));
      const loaded = await store.load(scope, rest).catch(() => ({ files: [], failed: [...rest] }));
      const files = new Map([...inMemory, ...loaded.files].map((item) => [item.id, item]));
      return {
        files: items.flatMap((item) => files.get(item.id) ?? []),
        unrestored: items.filter((item) => !files.has(item.id)),
      };
    },
    /** Forget and delete staged files the user removed, discarded, or submitted. */
    async remove(scope: string, ids: readonly string[]) {
      for (const id of ids) unsaved.delete(key(scope, id));
      revision.value += 1;
      await store.remove(scope, ids);
    },
  };
}

export type DraftFileRecovery = ReturnType<typeof createDraftFileRecovery>;

/** The application's one recovery queue; it holds no listeners or timers. */
export const draftFiles = createDraftFileRecovery();

const UNSAVED_PREFIX = "Couldn't save ",
  UNSAVED_SUFFIX =
    ' attached here, and saving retries with your next change. Recover or remove the affected files before submitting.';
export function unsavedDraftFilesMessage(names: readonly string[]) {
  const subject = names.length === 1 ? `“${names[0] ?? ''}”` : `${String(names.length)} files`,
    pronoun = names.length === 1 ? 'It stays' : 'They stay';
  return `${UNSAVED_PREFIX}${subject} for recovery if Hot Sheet closes. ${pronoun}${UNSAVED_SUFFIX}`;
}
export function unrestoredDraftFilesMessage(count: number) {
  return count === 1
    ? "Couldn't restore 1 staged file. It is kept. Reopen this project to retry, or remove the file before submitting."
    : `Couldn't restore ${String(count)} staged files. They are kept. Reopen this project to retry, or remove the files before submitting.`;
}
