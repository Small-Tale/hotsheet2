import { describe, expect, it, vi } from 'vitest';

import { createTicketWithAttachments, describeNewTicketAttachmentFailures } from './new-ticket-attachments';

describe('new ticket attachments', () => {
  it('creates once and uploads every staged file in order', async () => {
    const files = [new File(['one'], 'one.txt'), new File(['two'], 'two.txt')];
    const create = vi.fn(async () => ({ id: 'ticket', attachments: [] as string[] }));
    const upload = vi.fn(async (ticket: { id: string; attachments: string[] }, file: File) => ({ ...ticket, attachments: [...ticket.attachments, file.name] }));
    const result = await createTicketWithAttachments(files, create, upload);
    expect(create).toHaveBeenCalledOnce();
    expect(upload.mock.calls.map(([, file]) => file.name)).toEqual(['one.txt', 'two.txt']);
    expect(result).toEqual({ ticket: { id: 'ticket', attachments: ['one.txt', 'two.txt'] }, failed: [] });
  });

  it('keeps the created ticket and continues after a partial upload failure', async () => {
    const files = [new File(['bad'], 'bad.txt'), new File(['good'], 'good.txt')];
    const upload = vi.fn(async (ticket: { id: string; attachments: string[] }, file: File) => {
      if (file.name === 'bad.txt') throw new Error('upload rejected');
      return { ...ticket, attachments: [...ticket.attachments, file.name] };
    });
    const result = await createTicketWithAttachments(files, async () => ({ id: 'ticket', attachments: [] as string[] }), upload);
    expect(result.ticket.attachments).toEqual(['good.txt']);
    expect(result.failed).toEqual([{ name: 'bad.txt', reason: 'upload rejected' }]);
    expect(describeNewTicketAttachmentFailures(result.failed)).toContain('Ticket created, but “bad.txt” could not be attached');
  });

  it('does not upload anything when ticket creation fails', async () => {
    const upload = vi.fn();
    await expect(createTicketWithAttachments([new File(['proof'], 'proof.txt')], async () => { throw new Error('create failed'); }, upload)).rejects.toThrow('create failed');
    expect(upload).not.toHaveBeenCalled();
  });
});
