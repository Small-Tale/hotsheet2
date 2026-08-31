const MAX_MATERIALIZE_BYTES = 64 * 1024 * 1024;

export interface ScreenedAttachmentFiles {
  readable: File[];
  unreadable: string[];
}

async function materializeAttachment(file: File): Promise<File | undefined> {
  if (file.size === 0) return undefined;
  try {
    if (file.size > MAX_MATERIALIZE_BYTES) {
      const probe = await file.slice(0, 1).arrayBuffer();
      return probe.byteLength === 1 ? file : undefined;
    }
    const bytes = await file.arrayBuffer();
    if (bytes.byteLength !== file.size) return undefined;
    return new File([bytes], file.name, { type: file.type, lastModified: file.lastModified });
  } catch {
    return undefined;
  }
}

/** Materialize ordinary-sized browser files before fetch can lazily lose their backing data. */
export async function screenAttachmentFiles(files: readonly File[]): Promise<ScreenedAttachmentFiles> {
  const readable: File[] = [];
  const unreadable: string[] = [];
  for (const file of files) {
    const materialized = await materializeAttachment(file);
    if (materialized) readable.push(materialized);
    else unreadable.push(file.name || '(unnamed file)');
  }
  return { readable, unreadable };
}

export function describeUnreadableAttachments(names: readonly string[]): string {
  if (names.length === 0) return '';
  const listed = names.map(name => `“${name}”`).join(', ');
  const subject = names.length === 1 ? `${listed} has` : `${listed} have`;
  return `${subject} no readable content yet. A new macOS screen capture can be dragged before it has been written to disk. Wait for it to appear on the desktop, then add it again.`;
}
