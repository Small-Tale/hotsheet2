import { describe,expect,it,vi } from 'vitest';

import { Api, encodeAttachmentFilename } from './api';

describe('attachment filename transport',()=>{
  it('encodes macOS screenshot names as an ASCII-safe header value',()=>{
    const encoded=encodeAttachmentFilename('Screenshot 2026-08-31 at 8.49.09 AM.png');
    expect(encoded).toBe('Screenshot%202026-08-31%20at%208.49.09%E2%80%AFAM.png');
    expect(new TextEncoder().encode(encoded)).toHaveLength(encoded.length);
  });
});

describe('corrupt ticket transport',()=>{
  it('requests the checkout-scoped corrupt ticket collection',async()=>{
    const fetchMock=vi.spyOn(globalThis,'fetch').mockResolvedValue(new Response(JSON.stringify([{store:'local',store_path:'/store',path:'/store/ticket.md',error:'bad ticket'}]),{status:200}));
    await expect(new Api('/api').checkoutCorruptTickets('folder with spaces')).resolves.toHaveLength(1);
    expect(fetchMock).toHaveBeenCalledWith('/api/checkouts/folder%20with%20spaces/corrupt-tickets',expect.any(Object));
    fetchMock.mockRestore();
  });
});

describe('change polling transport',()=>{
  it('requests the secret-hiding project proxy with a cursor and abort signal',async()=>{
    const fetchMock=vi.spyOn(globalThis,'fetch').mockResolvedValue(new Response(JSON.stringify({cursor:8,events:[],overflow:false}),{status:200}));
    const controller=new AbortController();
    await expect(new Api('/api').pollEvents(7,controller.signal,1234)).resolves.toMatchObject({cursor:8});
    expect(fetchMock).toHaveBeenCalledWith('/api/ws/poll?timeout_ms=1234&since=7',expect.objectContaining({signal:controller.signal}));
    fetchMock.mockRestore();
  });
});

describe('ticket search transport',()=>{
  it('sends trimmed text through the comprehensive checkout query',async()=>{
    const fetchMock=vi.spyOn(globalThis,'fetch').mockResolvedValue(new Response('[]',{status:200}));
    await new Api('/api').checkoutTickets('folder with spaces','  HS2-QQRY00  ');
    expect(fetchMock).toHaveBeenCalledWith('/api/checkouts/folder%20with%20spaces/tickets?text=HS2-QQRY00',expect.any(Object));
    fetchMock.mockRestore();
  });
});
