import { DESKTOP_DRAFT_FILE_HEADERS, DESKTOP_DRAFT_FILES_PATH } from './desktop-storage-contract';

/**
 * Staged draft attachment bytes for the packaged desktop app (HS2-791W19). Each launch serves
 * the client from a new ephemeral loopback origin, so origin-scoped IndexedDB would forget them;
 * the desktop bridge keeps them in private native app-data records instead.
 */
const draftFileUrl = (key: string) => `${DESKTOP_DRAFT_FILES_PATH}?key=${encodeURIComponent(key)}`;

export async function saveDesktopDraftFile(key: string, file: File, fetcher: typeof fetch = fetch) {
  const response = await fetcher(draftFileUrl(key), {
    method: 'PUT',
    headers: {
      'Content-Type': 'application/octet-stream',
      [DESKTOP_DRAFT_FILE_HEADERS.name]: encodeURIComponent(file.name),
      [DESKTOP_DRAFT_FILE_HEADERS.type]: encodeURIComponent(file.type),
      [DESKTOP_DRAFT_FILE_HEADERS.lastModified]: String(Math.max(0, Math.trunc(file.lastModified))),
    },
    body: file,
  });
  if (!response.ok) throw new Error('Could not save a desktop draft file.');
}

export async function loadDesktopDraftFile(key: string, fetcher: typeof fetch = fetch): Promise<File | undefined> {
  const response = await fetcher(draftFileUrl(key), { cache: 'no-store' });
  if (response.status === 404) return undefined;
  if (!response.ok) throw new Error('Could not load a desktop draft file.');
  const header = (name: string) => decodeURIComponent(response.headers.get(name) ?? '');
  return new File([await response.blob()], header(DESKTOP_DRAFT_FILE_HEADERS.name), {
    type: header(DESKTOP_DRAFT_FILE_HEADERS.type),
    lastModified: Number(response.headers.get(DESKTOP_DRAFT_FILE_HEADERS.lastModified)),
  });
}

export async function deleteDesktopDraftFile(key: string, fetcher: typeof fetch = fetch) {
  const response = await fetcher(draftFileUrl(key), { method: 'DELETE' });
  if (!response.ok) throw new Error('Could not remove a desktop draft file.');
}
