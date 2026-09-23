import { describe, expect, it, vi } from 'vitest';

import { createTicketWithAttachments, describeNewTicketAttachmentFailures } from './new-ticket-attachments';

describe('new ticket attachments', () => {
  it('creates once and uploads every staged file in order', async () => {
    const files = [new File(['one'], 'one.txt'), new File(['two'], 'two.txt')];
    const create = vi.fn(async () => ({ id: 'ticket', attachments: [] as string[] }));
    const upload = vi.fn(async (ticket: { id: string; attachments: string[] }, file: File) => ({
      ...ticket,
      attachments: [...ticket.attachments, file.name],
    }));
    const result = await createTicketWithAttachments(files, create, upload);
    expect(create).toHaveBeenCalledOnce();
    expect(upload.mock.calls.map(([, file]) => file.name)).toEqual(['one.txt', 'two.txt']);
    expect(result).toEqual({ ticket: { id: 'ticket', attachments: ['one.txt', 'two.txt'] }, failed: [] });
  });

  it('does not start the next upload or settle the batch while the current upload is delayed', async () => {
    let releaseFirst: () => void = () => undefined;
    const firstGate = new Promise<void>((resolve) => {
        releaseFirst = resolve;
      }),
      uploads: string[] = [];
    let settled = false;
    const batch = createTicketWithAttachments(
      [new File(['one'], 'one.txt'), new File(['two'], 'two.txt')],
      async () => ({ id: 'ticket' }),
      async (ticket, file) => {
        uploads.push(file.name);
        if (file.name === 'one.txt') await firstGate;
        return ticket;
      },
    ).finally(() => {
      settled = true;
    });

    await vi.waitFor(() => {
      expect(uploads).toEqual(['one.txt']);
    });
    expect(settled).toBe(false);
    releaseFirst();
    await batch;
    expect(uploads).toEqual(['one.txt', 'two.txt']);
    expect(settled).toBe(true);
  });

  it('projects the created ticket before attachment uploads settle', async () => {
    const order: string[] = [];
    await createTicketWithAttachments(
      [new File(['proof'], 'proof.txt')],
      async () => {
        order.push('created');
        return { id: 'ticket' };
      },
      async (ticket) => {
        order.push('uploaded');
        return ticket;
      },
      (ticket) => {
        order.push(`projected:${ticket.id}`);
      },
    );
    expect(order).toEqual(['created', 'projected:ticket', 'uploaded']);
  });

  it('keeps the created ticket and continues after a partial upload failure', async () => {
    const files = [new File(['bad'], 'bad.txt'), new File(['good'], 'good.txt')];
    const upload = vi.fn(async (ticket: { id: string; attachments: string[] }, file: File) => {
      if (file.name === 'bad.txt') throw new Error('upload rejected');
      return { ...ticket, attachments: [...ticket.attachments, file.name] };
    });
    const result = await createTicketWithAttachments(
      files,
      async () => ({ id: 'ticket', attachments: [] as string[] }),
      upload,
    );
    expect(result.ticket.attachments).toEqual(['good.txt']);
    expect(result.failed).toEqual([{ name: 'bad.txt', reason: 'upload rejected' }]);
    expect(describeNewTicketAttachmentFailures(result.failed)).toContain(
      'Ticket created, but “bad.txt” could not be attached',
    );
  });

  it('does not upload anything when ticket creation fails', async () => {
    const upload = vi.fn();
    await expect(
      createTicketWithAttachments(
        [new File(['proof'], 'proof.txt')],
        async () => {
          throw new Error('create failed');
        },
        upload,
      ),
    ).rejects.toThrow('create failed');
    expect(upload).not.toHaveBeenCalled();
  });
});
