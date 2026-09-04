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
    const fetchMock=vi.spyOn(globalThis,'fetch').mockImplementation(async()=>new Response(JSON.stringify([{store:'local',store_path:'/store',path:'/store/ticket.md',error:'bad ticket'}]),{status:200}));
    await expect(new Api('/api').checkoutCorruptTickets('folder with spaces')).resolves.toHaveLength(1);
    expect(fetchMock).toHaveBeenCalledWith('/api/checkouts/folder%20with%20spaces/corrupt-tickets',expect.any(Object));
    await new Api('/api').createCorruptTicketRepair('folder with spaces','/tmp/broken.md');
    expect(fetchMock).toHaveBeenLastCalledWith('/api/checkouts/folder%20with%20spaces/corrupt-tickets/repair',expect.objectContaining({method:'POST',body:'{"path":"/tmp/broken.md"}'}));
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

describe('terminal dashboard transport',()=>{
  it('lists terminals, reads a safely encoded snapshot, and creates a project terminal',async()=>{
    const fetchMock=vi.spyOn(globalThis,'fetch')
      .mockResolvedValueOnce(new Response(JSON.stringify([{id:'agent/1',alive:true,busy:false}]),{status:200}))
      .mockResolvedValueOnce(new Response(JSON.stringify({id:'agent/1',alive:true,busy:false,scrollback:'ready'}),{status:200}))
      .mockResolvedValueOnce(new Response(JSON.stringify({id:'terminal-2',alive:true,busy:false}),{status:200}));
    const api=new Api('/api');
    await expect(api.terminals()).resolves.toHaveLength(1);
    await expect(api.terminal('agent/1')).resolves.toMatchObject({scrollback:'ready'});
    await expect(api.createTerminal({cwd:'/project root'})).resolves.toMatchObject({id:'terminal-2'});
    expect(fetchMock).toHaveBeenNthCalledWith(1,'/api/terminals',expect.any(Object));
    expect(fetchMock).toHaveBeenNthCalledWith(2,'/api/terminals/agent%2F1',expect.any(Object));
    expect(fetchMock).toHaveBeenNthCalledWith(3,'/api/terminals',expect.objectContaining({method:'POST',body:'{"cwd":"/project root"}'}));
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

describe('checkout bulk update transport',()=>{
  it('sends every selected ticket through one batch request',async()=>{
    const fetchMock=vi.spyOn(globalThis,'fetch').mockResolvedValue(new Response('[]',{status:200}));
    await new Api('/api').batchUpdateCheckoutTickets('folder with spaces',[
      {id:'one',patch:{status:'verified',expected_token:'token-1'}},
      {id:'two',patch:{priority:'urgent',expected_token:'token-2'}},
    ]);
    expect(fetchMock).toHaveBeenCalledTimes(1);
    expect(fetchMock).toHaveBeenCalledWith('/api/checkouts/folder%20with%20spaces/batch',expect.objectContaining({
      method:'POST',
      body:'{"updates":[{"id":"one","status":"verified","expected_token":"token-1"},{"id":"two","priority":"highest","expected_token":"token-2"}]}',
    }));
    fetchMock.mockRestore();
  });
});

describe('ticket code review transport',()=>{
  it('reads review targets and accepts an empty successful launch response',async()=>{
    const fetchMock=vi.spyOn(globalThis,'fetch')
      .mockResolvedValueOnce(new Response(JSON.stringify({commits:[],ranges:[],truncated:false}),{status:200}))
      .mockResolvedValueOnce(new Response(null,{status:204}));
    const api=new Api('/api');
    await api.codeReview('folder with spaces','ticket/1');
    await expect(api.openCodeReview('folder with spaces','ticket/1',{mode:'commit',commit:'abc'})).resolves.toBeUndefined();
    expect(fetchMock).toHaveBeenNthCalledWith(1,'/api/checkouts/folder%20with%20spaces/tickets/ticket%2F1/code-review',expect.any(Object));
    expect(fetchMock).toHaveBeenNthCalledWith(2,'/api/checkouts/folder%20with%20spaces/tickets/ticket%2F1/code-review',expect.objectContaining({method:'POST',body:'{"mode":"commit","commit":"abc"}'}));
    fetchMock.mockRestore();
  });
});

describe('atomic Not Working transport',()=>{
  it('sends note, evidence, and concurrency token in one multipart request',async()=>{
    const fetchMock=vi.spyOn(globalThis,'fetch').mockResolvedValue(new Response(JSON.stringify({status:'not_started',up_next:true}),{status:200}));
    const proof=new File(['proof'],'proof ünicode.txt',{type:'text/plain'});
    await new Api('/api').reportNotWorking('git local','ticket/1',' regressed ',[proof],'token-1');
    const [url,init]=fetchMock.mock.calls[0] as [string,RequestInit];
    expect(url).toBe('/api/providers/git%20local/tickets/ticket%2F1/not-working');
    expect(init.method).toBe('POST');
    expect(init.body).toBeInstanceOf(FormData);
    const body=init.body as FormData;
    expect(body.get('note')).toBe('regressed');
    expect(body.get('expected_token')).toBe('token-1');
    expect((body.get('evidence') as File).name).toBe('proof ünicode.txt');
    expect((init.headers as Headers).has('Content-Type')).toBe(false);
    fetchMock.mockRestore();
  });
});
