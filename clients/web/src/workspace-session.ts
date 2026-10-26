import { browserDraftFileRecord, fileFromBrowserDraftRecord } from './browser-draft-file-contract';
import { createBrowserDraftMutationQueue } from './browser-draft-mutation-queue';
import type { InspectorTab } from './components/ticket-inspector';
import { deleteDesktopDraftFile, loadDesktopDraftFile, saveDesktopDraftFile } from './desktop-draft-files';
import type { InlineFeedbackReply } from './feedback-replies';
import type { TicketView } from './ticket-views';

export interface StoredPendingAttachment {
  id: string;
  name: string;
}
export interface ProjectWorkspaceSession {
  selectedView: TicketView;
  selectedTicketSlugs: string[];
  searchOpen: boolean;
  searchQuery: string;
  inspectorTab: InspectorTab;
  readerTab: InspectorTab;
  composer: {
    open: boolean;
    title: string;
    details: string;
    category: string;
    upNext: boolean;
    attachments: StoredPendingAttachment[];
  };
  composingNote: boolean;
  newNoteDraft: string;
  feedbackReplies: Record<string, InlineFeedbackReply[]>;
  feedbackSelections: Record<string, string[]>;
  feedbackNoteId?: string;
  feedbackDraft: string;
  notWorking?: {
    ticketId: string;
    slug: string;
    connectionId: string;
    mode?: 'not-working' | 'reopen';
    note: string;
    attachments: StoredPendingAttachment[];
  };
}

const PREFIX = 'hotsheet.workspace.project-session.v1.';
const ACTIVE_ROOT = 'hotsheet.workspace.active-project-root.v1';
const HS1_MIGRATION_DISMISSED = 'hotsheet.workspace.hs1-migration-dismissed.v1.';
const HS1_CLEANUP_DISMISSED = 'hotsheet.workspace.hs1-cleanup-dismissed.v1.';
const DB_NAME = 'hotsheet-draft-files-v1',
  STORE = 'files';
const mutateBrowserDraft = createBrowserDraftMutationQueue();

const stringValue = (value: unknown, fallback = '') => (typeof value === 'string' ? value : fallback);
const booleanValue = (value: unknown) => value === true;
function recordValue(value: unknown): Record<string, unknown> {
  return value && typeof value === 'object' && !Array.isArray(value) ? (value as Record<string, unknown>) : {};
}
function validAttachments(value: unknown): StoredPendingAttachment[] {
  return Array.isArray(value)
    ? value.flatMap((item) => {
        const record = recordValue(item),
          id = stringValue(record.id),
          name = stringValue(record.name);
        return id && name ? [{ id, name }] : [];
      })
    : [];
}
function validRecord(value: unknown): Record<string, InlineFeedbackReply[]> {
  return Object.fromEntries(
    Object.entries(recordValue(value)).map(
      ([key, replies]) =>
        [
          key,
          Array.isArray(replies)
            ? replies.flatMap((reply) => {
                const record = recordValue(reply);
                return Number.isFinite(record.offset) && typeof record.text === 'string'
                  ? [{ offset: Number(record.offset), text: record.text }]
                  : [];
              })
            : [],
        ] as const,
    ),
  );
}
function validSelections(value: unknown): Record<string, string[]> {
  return Object.fromEntries(
    Object.entries(recordValue(value)).map(
      ([key, items]) =>
        [key, Array.isArray(items) ? items.filter((item): item is string => typeof item === 'string') : []] as const,
    ),
  );
}

export function loadProjectWorkspaceSession(
  storage: Pick<Storage, 'getItem'>,
  projectId: string,
): ProjectWorkspaceSession | undefined {
  try {
    const value = recordValue(JSON.parse(storage.getItem(PREFIX + projectId) ?? 'null'));
    const composer = recordValue(value.composer);
    if (Object.keys(value).length === 0 || Object.keys(composer).length === 0) return undefined;
    const selectedView = stringValue(value.selectedView),
      inspectorTab = stringValue(value.inspectorTab),
      readerTab = stringValue(value.readerTab),
      notWorking = recordValue(value.notWorking);
    return {
      selectedView: (['all', 'backlog', 'archive', 'trash', 'errors'].includes(selectedView) ||
      /^custom:[a-z0-9][a-z0-9-]{0,63}$/.test(selectedView)
        ? selectedView
        : 'all') as TicketView,
      selectedTicketSlugs: Array.isArray(value.selectedTicketSlugs)
        ? value.selectedTicketSlugs.filter((item): item is string => typeof item === 'string')
        : [],
      searchOpen: booleanValue(value.searchOpen),
      searchQuery: stringValue(value.searchQuery),
      inspectorTab: (['info', 'timeline', 'attachments', 'code-review'].includes(inspectorTab)
        ? inspectorTab
        : 'info') as InspectorTab,
      readerTab: (['info', 'timeline', 'attachments', 'code-review'].includes(readerTab)
        ? readerTab
        : 'info') as InspectorTab,
      composer: {
        open: booleanValue(composer.open),
        title: stringValue(composer.title),
        details: stringValue(composer.details),
        category: stringValue(composer.category, 'task'),
        upNext: booleanValue(composer.upNext),
        attachments: validAttachments(composer.attachments),
      },
      composingNote: booleanValue(value.composingNote),
      newNoteDraft: stringValue(value.newNoteDraft),
      feedbackReplies: validRecord(value.feedbackReplies),
      feedbackSelections: validSelections(value.feedbackSelections),
      feedbackNoteId: typeof value.feedbackNoteId === 'string' ? value.feedbackNoteId : undefined,
      feedbackDraft: stringValue(value.feedbackDraft),
      notWorking: Object.keys(notWorking).length
        ? {
            ticketId: stringValue(notWorking.ticketId),
            slug: stringValue(notWorking.slug),
            connectionId: stringValue(notWorking.connectionId),
            mode: notWorking.mode === 'reopen' ? 'reopen' : 'not-working',
            note: stringValue(notWorking.note),
            attachments: validAttachments(notWorking.attachments),
          }
        : undefined,
    };
  } catch {
    return undefined;
  }
}

export function saveProjectWorkspaceSession(
  storage: Pick<Storage, 'setItem'>,
  projectId: string,
  value: ProjectWorkspaceSession,
) {
  storage.setItem(PREFIX + projectId, JSON.stringify(value));
}
export function activeProjectRoot(storage: Pick<Storage, 'getItem'>) {
  return storage.getItem(ACTIVE_ROOT) ?? '';
}
export function saveActiveProjectRoot(storage: Pick<Storage, 'setItem'>, root: string) {
  storage.setItem(ACTIVE_ROOT, root);
}
export function hs1MigrationPromptDismissed(
  storage: Pick<Storage, 'getItem'>,
  projectId: string,
  sourceIdentity: string,
) {
  return Boolean(sourceIdentity) && storage.getItem(HS1_MIGRATION_DISMISSED + projectId) === sourceIdentity;
}
export function dismissHs1MigrationPrompt(
  storage: Pick<Storage, 'setItem'>,
  projectId: string,
  sourceIdentity: string,
) {
  if (sourceIdentity) storage.setItem(HS1_MIGRATION_DISMISSED + projectId, sourceIdentity);
}
export function hs1CleanupPromptDismissed(
  storage: Pick<Storage, 'getItem'>,
  projectId: string,
  sourceIdentity: string,
) {
  return Boolean(sourceIdentity) && storage.getItem(HS1_CLEANUP_DISMISSED + projectId) === sourceIdentity;
}
export function dismissHs1CleanupPrompt(storage: Pick<Storage, 'setItem'>, projectId: string, sourceIdentity: string) {
  if (sourceIdentity) storage.setItem(HS1_CLEANUP_DISMISSED + projectId, sourceIdentity);
}

function database(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(DB_NAME, 1);
    request.onupgradeneeded = () => {
      request.result.createObjectStore(STORE);
    };
    request.onsuccess = () => {
      resolve(request.result);
    };
    request.onerror = () => {
      reject(request.error ?? new Error('Could not open the draft file database.'));
    };
  });
}
/** The packaged desktop shell's bridge owns draft bytes; browsers keep origin-scoped IndexedDB. */
function desktopDraftFiles() {
  return typeof window !== 'undefined' && '__HOTSHEET_DESKTOP_STORAGE__' in window;
}
export async function saveDraftFile(scope: string, id: string, file: File) {
  if (desktopDraftFiles()) {
    await saveDesktopDraftFile(`${scope}:${id}`, file);
    return;
  }
  await mutateBrowserDraft(`${scope}:${id}`, async () => {
    // Prepare bytes before opening the transaction: awaiting inside it can make it inactive.
    const record = await browserDraftFileRecord(file),
      db = await database();
    try {
      await transaction(db, 'readwrite', (store) => {
        store.put(record, `${scope}:${id}`);
      });
    } finally {
      db.close();
    }
  });
}
export interface StoredDraftFile {
  id: string;
  name: string;
  file: File;
}
/**
 * Load each staged file independently (HS2-791W19): `files` were found, absent copies are
 * dropped, and `failed` references could not be read now and must be kept for another try.
 */
export async function loadDraftFiles(
  scope: string,
  items: readonly StoredPendingAttachment[],
): Promise<{ files: StoredDraftFile[]; failed: StoredPendingAttachment[] }> {
  if (!items.length) return { files: [], failed: [] };
  let read: (item: StoredPendingAttachment) => Promise<StoredDraftFile | undefined>, db: IDBDatabase | undefined;
  if (desktopDraftFiles())
    read = async (item) => {
      const file = await loadDesktopDraftFile(`${scope}:${item.id}`);
      return file ? { id: item.id, name: file.name, file } : undefined;
    };
  else {
    const opened = await database();
    db = opened;
    read = async (item) => {
      const value = await requestValue<unknown>(
        opened.transaction(STORE).objectStore(STORE).get(`${scope}:${item.id}`),
      );
      if (value === undefined) return undefined;
      const file = fileFromBrowserDraftRecord(value);
      return { id: item.id, name: file.name, file };
    };
  }
  const results = await Promise.allSettled(items.map(read));
  db?.close();
  const files: StoredDraftFile[] = [],
    failed: StoredPendingAttachment[] = [];
  results.forEach((result, index) => {
    if (result.status === 'rejected') failed.push(items[index]);
    else if (result.value) files.push(result.value);
  });
  return { files, failed };
}
export async function deleteDraftFiles(scope: string, ids: readonly string[]) {
  if (!ids.length) return;
  if (desktopDraftFiles()) {
    await Promise.all(ids.map((id) => deleteDesktopDraftFile(`${scope}:${id}`)));
    return;
  }
  await Promise.all(
    ids.map((id) =>
      mutateBrowserDraft(`${scope}:${id}`, async () => {
        const db = await database();
        try {
          await transaction(db, 'readwrite', (store) => {
            store.delete(`${scope}:${id}`);
          });
        } finally {
          db.close();
        }
      }),
    ),
  );
}
function requestValue<T>(request: IDBRequest): Promise<T | undefined> {
  return new Promise((resolve, reject) => {
    request.onsuccess = () => {
      resolve(request.result as T | undefined);
    };
    request.onerror = () => {
      reject(request.error ?? new Error('Could not read a draft file.'));
    };
  });
}
function transaction(
  db: IDBDatabase,
  mode: IDBTransactionMode,
  action: (store: IDBObjectStore) => void,
): Promise<void> {
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE, mode);
    action(tx.objectStore(STORE));
    tx.oncomplete = () => {
      resolve();
    };
    tx.onerror = () => {
      reject(tx.error ?? new Error('Could not update draft files.'));
    };
  });
}
