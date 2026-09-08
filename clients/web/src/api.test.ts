import { describe,expect,it,vi } from 'vitest';

import { Api, encodeAttachmentFilename, turnStreamEvents } from './api';

describe('attachment filename transport',()=>{
  it('encodes macOS screenshot names as an ASCII-safe header value',()=>{
    const encoded=encodeAttachmentFilename('Screenshot 2026-08-31 at 8.49.09 AM.png');
    expect(encoded).toBe('Screenshot%202026-08-31%20at%208.49.09%E2%80%AFAM.png');
    expect(new TextEncoder().encode(encoded)).toHaveLength(encoded.length);
  });
  it('encodes filename references and host actions on checkout routes',async()=>{
    const fetchMock=vi.spyOn(globalThis,'fetch').mockResolvedValue(new Response(JSON.stringify({path:'/store/screen shot.svg'}),{status:200}));
    const api=new Api('/api');
    expect(api.checkoutAttachmentByNameUrl('folder one','HS2-ONE','screen shot.svg')).toBe('/api/checkouts/folder%20one/tickets/HS2-ONE/attachments/by-name/screen%20shot.svg');
    await expect(api.checkoutAttachmentByNameAction('folder one','HS2-ONE','screen shot.svg','reveal')).resolves.toEqual({path:'/store/screen shot.svg'});
    expect(fetchMock).toHaveBeenCalledWith('/api/checkouts/folder%20one/tickets/HS2-ONE/attachments/by-name/screen%20shot.svg/action',expect.objectContaining({method:'POST',body:'{"action":"reveal"}'}));
    fetchMock.mockRestore();
  });
  it('stores normalized media annotations on the attachment route',async()=>{
    const annotation={id:'annotation-1',x:1000,y:2000,width:3000,height:2500,start_ms:1000,end_ms:2000,text:'Check this'};
    const fetchMock=vi.spyOn(globalThis,'fetch').mockResolvedValue(new Response(JSON.stringify({store:'git-local',ticket:{attachments:[]}}),{status:200}));
    await new Api('/api').updateCheckoutAttachmentAnnotations('folder','ticket','attachment',[annotation]);
    expect(fetchMock).toHaveBeenCalledWith('/api/checkouts/folder/tickets/ticket/attachments/attachment',expect.objectContaining({method:'PUT',body:JSON.stringify({annotations:[annotation]})}));
    fetchMock.mockRestore();
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
  it('projects known turn events and safely carries unknown newer event types',()=>{
    const response={cursor:12,overflow:false,events:[
      {cursor:11,store:'s',kind:'turn_event',id:'c',slug:'codex',turn:{connection_id:'c',event:{type:'output',content:'hello',truncated:false}}},
      {cursor:12,store:'s',kind:'turn_event',id:'c',slug:'codex',turn:{connection_id:'c',event:{type:'future_server_event',value:1}}},
    ]};
    expect(turnStreamEvents(response).map(item=>item.event.type)).toEqual(['output','future_server_event']);
  });
});

describe('client-owned AI drive transport',()=>{
  it('starts a connection, sends a resumable turn, and interrupts by advertised action',async()=>{
    const connection={id:'main/1',tool:'codex',project:'/project',role:'main',busy:false,actions:['send_turn','interrupt']};
    const fetchMock=vi.spyOn(globalThis,'fetch').mockImplementation(async()=>new Response(JSON.stringify(connection),{status:202}));
    const api=new Api('/api');
    await api.createToolConnection({tool:'codex',connection_id:'main/1'});
    await api.sendToolTurn('main/1','Continue this work','thread/1');
    await api.interruptToolTurn('main/1');
    expect(fetchMock).toHaveBeenNthCalledWith(1,'/api/drive/connections',expect.objectContaining({method:'POST',body:'{"tool":"codex","connection_id":"main/1"}'}));
    expect(fetchMock).toHaveBeenNthCalledWith(2,'/api/drive/connections/main%2F1/turns',expect.objectContaining({method:'POST',body:'{"content":"Continue this work","session_id":"thread/1"}'}));
    expect(fetchMock).toHaveBeenNthCalledWith(3,'/api/drive/connections/main%2F1/interrupt',expect.objectContaining({method:'POST'}));
    fetchMock.mockRestore();
  });
});

describe('provider onboarding transport',()=>{
  it('creates a non-secret connection and links it as the checkout default',async()=>{
    const connection={id:'github-main',provider:'github',locator:'small-tale/hotsheet2',name:'GitHub Issues',default:true,settings:{credential:{secret:'github-small-tale'}}};
    const fetchMock=vi.spyOn(globalThis,'fetch')
      .mockResolvedValueOnce(new Response(JSON.stringify(connection),{status:201}))
      .mockResolvedValueOnce(new Response(JSON.stringify({id:'checkout',root:'/work',alias:'work',stores:[]}),{status:200}));
    const api=new Api('/api');
    await api.createConnection(connection);
    await api.addCheckoutSource('folder with spaces',connection,true);
    expect(fetchMock).toHaveBeenNthCalledWith(1,'/api/provider-connections',expect.objectContaining({method:'POST',body:JSON.stringify(connection)}));
    expect(fetchMock).toHaveBeenNthCalledWith(2,'/api/checkouts/folder%20with%20spaces/sources/github-main',expect.objectContaining({method:'PUT',body:'{"provider":"github","locator":"small-tale/hotsheet2","make_default":true}'}));
    fetchMock.mockRestore();
  });
  it('keeps GitHub device credentials on the server while starting, waiting, and cancelling',async()=>{
    const started={session_id:'auth-1',user_code:'ABCD-EFGH',verification_uri:'https://github.test/login/device',expires_in:900};
    const authorized={state:'authorized',credential_reference:'github-app-auth-1'};
    const fetchMock=vi.spyOn(globalThis,'fetch')
      .mockResolvedValueOnce(new Response(JSON.stringify(started),{status:202}))
      .mockResolvedValueOnce(new Response(JSON.stringify(authorized),{status:200}))
      .mockResolvedValueOnce(new Response(JSON.stringify({repositories:['small-tale/hotsheet2']}),{status:200}))
      .mockResolvedValueOnce(new Response(null,{status:204}));
    const api=new Api('/api');
    await expect(api.startGitHubAuth('https://github.test')).resolves.toEqual(started);
    await expect(api.waitGitHubAuth('auth-1')).resolves.toEqual(authorized);
    await expect(api.githubAuthRepositories('auth-1')).resolves.toEqual({repositories:['small-tale/hotsheet2']});
    await expect(api.cancelGitHubAuth('auth-1')).resolves.toBeUndefined();
    expect(fetchMock).toHaveBeenNthCalledWith(1,'/api/github-auth/device',expect.objectContaining({method:'POST',body:'{"web_base":"https://github.test"}'}));
    expect(fetchMock).toHaveBeenNthCalledWith(2,'/api/github-auth/device/auth-1',expect.any(Object));
    expect(fetchMock).toHaveBeenNthCalledWith(3,'/api/github-auth/device/auth-1/repositories',expect.any(Object));
    expect(fetchMock).toHaveBeenNthCalledWith(4,'/api/github-auth/device/auth-1',expect.objectContaining({method:'DELETE'}));
    expect(JSON.stringify(fetchMock.mock.calls)).not.toContain('access_token');
    fetchMock.mockRestore();
  });
});

describe('terminal dashboard transport',()=>{
  it('lists terminals, reads a safely encoded snapshot, and creates a project terminal',async()=>{
    const fetchMock=vi.spyOn(globalThis,'fetch')
      .mockResolvedValueOnce(new Response(JSON.stringify([{id:'agent/1',alive:true,busy:false}]),{status:200}))
      .mockResolvedValueOnce(new Response(JSON.stringify({id:'agent/1',alive:true,busy:false,scrollback:'ready'}),{status:200}))
      .mockResolvedValueOnce(new Response(JSON.stringify({id:'terminal-2',alive:true,busy:false}),{status:200}))
      .mockResolvedValueOnce(new Response(null,{status:204}));
    const api=new Api('/api');
    await expect(api.terminals()).resolves.toHaveLength(1);
    await expect(api.terminal('agent/1')).resolves.toMatchObject({scrollback:'ready'});
    await expect(api.createTerminal({cwd:'/project root'})).resolves.toMatchObject({id:'terminal-2'});
    await expect(api.deleteTerminal('agent/1')).resolves.toBeUndefined();
    expect(fetchMock).toHaveBeenNthCalledWith(1,'/api/terminals',expect.any(Object));
    expect(fetchMock).toHaveBeenNthCalledWith(2,'/api/terminals/agent%2F1',expect.any(Object));
    expect(fetchMock).toHaveBeenNthCalledWith(3,'/api/terminals',expect.objectContaining({method:'POST',body:'{"cwd":"/project root"}'}));
    expect(fetchMock).toHaveBeenNthCalledWith(4,'/api/terminals/agent%2F1',expect.objectContaining({method:'DELETE'}));
    fetchMock.mockRestore();
  });
});

describe('terminal settings transport',()=>{
  it('keeps the global-history opt-out behind a machine-local host API',async()=>{const fetchMock=vi.spyOn(globalThis,'fetch').mockImplementation(async()=>new Response('{"inherit_global_shell_history":true}',{status:200}));const api=new Api('/api');await expect(api.terminalSettings()).resolves.toEqual({inherit_global_shell_history:true});await api.saveTerminalSettings({inherit_global_shell_history:false});expect(fetchMock).toHaveBeenNthCalledWith(1,'/api/terminal-settings',expect.any(Object));expect(fetchMock).toHaveBeenNthCalledWith(2,'/api/terminal-settings',expect.objectContaining({method:'PUT',body:'{"inherit_global_shell_history":false}'}));fetchMock.mockRestore()});
});

describe('ticket search transport',()=>{
  it('sends trimmed text through the comprehensive checkout query',async()=>{
    const fetchMock=vi.spyOn(globalThis,'fetch').mockResolvedValue(new Response('[]',{status:200}));
    await new Api('/api').checkoutTickets('folder with spaces','  HS2-QQRY00  ');
    expect(fetchMock).toHaveBeenCalledWith('/api/checkouts/folder%20with%20spaces/tickets?text=HS2-QQRY00',expect.any(Object));
    fetchMock.mockRestore();
  });
  it('requests non-compact provider-indexed rows for advanced search',async()=>{
    const fetchMock=vi.spyOn(globalThis,'fetch').mockResolvedValue(new Response('[]',{status:200}));
    await new Api('/api').checkoutTickets('demo',{text:' referenced ticket ',compact:false,up_next:true});
    expect(fetchMock).toHaveBeenCalledWith('/api/checkouts/demo/tickets?text=referenced+ticket&compact=false&up_next=true',expect.any(Object));
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

describe('repository browser transport',()=>{
  it('sends host file and review actions through checkout routes',async()=>{
    const fetchMock=vi.spyOn(globalThis,'fetch').mockResolvedValue(new Response(null,{status:204}));
    const api=new Api('/api');
    await api.repositoryFileAction('folder with spaces','src/a b.ts','reveal');
    await api.openRepositoryReview('folder with spaces',{mode:'range',from:'aaa',to:'bbb'});
    expect(fetchMock).toHaveBeenNthCalledWith(1,'/api/checkouts/folder%20with%20spaces/repository/files/action',expect.objectContaining({method:'POST',body:'{"path":"src/a b.ts","action":"reveal"}'}));
    expect(fetchMock).toHaveBeenNthCalledWith(2,'/api/checkouts/folder%20with%20spaces/repository/review',expect.objectContaining({method:'POST',body:'{"mode":"range","from":"aaa","to":"bbb"}'}));
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
