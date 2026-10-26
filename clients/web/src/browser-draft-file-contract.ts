import { isRecord } from './json-value';

export type BrowserDraftFileRecord = {
  name: string;
  type: string;
  lastModified: number;
} & ({ blob: ArrayBuffer; size: number } | { blob: Blob; size?: number });

/** HS2-2HF3H4: WebKit can reject IndexedDB Blob/File writes on insecure LAN origins. */
export async function browserDraftFileRecord(file: File): Promise<BrowserDraftFileRecord> {
  return {
    // Preserve the key: already-open older clients also accept ArrayBuffer in File([record.blob]).
    blob: await file.arrayBuffer(),
    name: file.name,
    type: file.type,
    lastModified: file.lastModified,
    size: file.size,
  };
}

/** Read old Blob/File records without rewriting them; unknown shapes remain recoverable failures. */
export function fileFromBrowserDraftRecord(value: unknown): File {
  if (
    !isRecord(value) ||
    typeof value.name !== 'string' ||
    typeof value.type !== 'string' ||
    typeof value.lastModified !== 'number' ||
    !Number.isSafeInteger(value.lastModified)
  )
    throw new Error('Invalid browser draft file record.');
  let content: Blob | ArrayBuffer;
  if (value.blob instanceof ArrayBuffer) {
    if (value.size !== value.blob.byteLength) throw new Error('Invalid browser draft file bytes.');
    content = value.blob;
  } else {
    if (!(value.blob instanceof Blob) || ('size' in value && value.size !== value.blob.size))
      throw new Error('Invalid legacy browser draft file.');
    content = value.blob;
  }
  return new File([content], value.name, { type: value.type, lastModified: value.lastModified });
}
