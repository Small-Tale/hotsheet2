export interface FailedNewTicketAttachment {
  name: string;
  reason: string;
}

export interface NewTicketAttachmentResult<T> {
  ticket: T;
  failed: FailedNewTicketAttachment[];
}

/** Create first, then upload every staged file without hiding partial upload failures. */
export async function createTicketWithAttachments<T>(
  files: readonly File[],
  create: () => Promise<T>,
  upload: (ticket: T, file: File) => Promise<T>,
  onCreated?: (ticket: T) => void,
): Promise<NewTicketAttachmentResult<T>> {
  let ticket = await create();
  onCreated?.(ticket);
  const failed: FailedNewTicketAttachment[] = [];
  for (const file of files) {
    try {
      ticket = await upload(ticket, file);
    } catch (reason) {
      failed.push({
        name: file.name || '(unnamed file)',
        reason: reason instanceof Error ? reason.message : String(reason),
      });
    }
  }
  return { ticket, failed };
}

export function describeNewTicketAttachmentFailures(
  failures: readonly FailedNewTicketAttachment[],
): string {
  if (failures.length === 0) return '';
  const names = failures.map(({ name }) => `“${name}”`).join(', ');
  return `Ticket created, but ${names} could not be attached. Add the ${failures.length === 1 ? 'file' : 'files'} again from the ticket’s Attachments tab.`;
}
