import { expect, type Locator,test } from '@playwright/test';

import { expectResponsiveFeedbackRectangle, measureFeedbackRectangle } from './dev-review-performance';

const project = { id:'demo-checkout', root:'/work/demo', name:'demo', stores:['/work/demo.hs2'], apiPath:'/__hotsheet/project-api/demo-checkout' };
const row = { connection_id:'git-local', native_id:'01', qualified_id:'git-local:01', id:'01', slug:'HS2-DEMO01', title:'Use real project tickets', category:'feature', priority:'high', status:'started', up_next:true, tags:['client'], blocked_by:[], claim_count:0, created_at:'2026-08-30T00:00:00Z', updated_at:'2026-08-30T01:00:00Z' };
const backlogRow = { ...row, native_id:'03', qualified_id:'git-local:03', id:'03', slug:'HS2-BACK01', title:'Deferred backlog ticket', status:'backlog', up_next:false };
const archiveRow = { ...row, native_id:'04', qualified_id:'git-local:04', id:'04', slug:'HS2-ARCH01', title:'Archived ticket', status:'archive', up_next:false };
const deletedRow = { ...row, native_id:'12', qualified_id:'git-local:12', id:'12', slug:'HS2-DEL001', title:'Deleted ticket', status:'deleted', up_next:false };
const movedRow = { ...row, native_id:'13', qualified_id:'git-local:13', id:'13', slug:'HS2-MOVED1', title:'Moved ticket', status:'moved', up_next:false };
const notStartedRow = { ...row, native_id:'05', qualified_id:'git-local:05', id:'05', slug:'HS2-NEXT01', title:'Not started ticket', status:'not_started', up_next:false };
const completedRow = { ...row, native_id:'06', qualified_id:'git-local:06', id:'06', slug:'HS2-DONE01', title:'Completed ticket', status:'completed', up_next:false };
const verifiedRow = { ...row, native_id:'07', qualified_id:'git-local:07', id:'07', slug:'HS2-VERIFY01', title:'Verified ticket', status:'verified', up_next:false };
const startedRow2 = { ...row, native_id:'08', qualified_id:'git-local:08', id:'08', slug:'HS2-START02', title:'Second started ticket', status:'started', up_next:false };
const startedRow3 = { ...row, native_id:'09', qualified_id:'git-local:09', id:'09', slug:'HS2-START03', title:'Third started ticket', status:'started', up_next:false };
const searchSlugRow = { ...row, native_id:'10', qualified_id:'git-local:10', id:'10', slug:'HS2-QQRY00', title:'Parser boundary bug', status:'started', up_next:false };
const searchDetailsRow = { ...row, native_id:'11', qualified_id:'git-local:11', id:'11', slug:'HS2-SHG7YS', title:'Unrelated visible title', status:'started', up_next:false };
const full = { ...row, details:'The real ticket body. [Project guide](/docs/project-guide)', blocked_reason:null, concurrency_token:'token', notes:[{id:'N1',kind:'activity',created_at:'2026-08-30T00:30:00Z',edited_at:'2026-08-30T00:30:00Z',text:'Connected the client\nLoaded checkout-scoped tickets.'},{id:'N2',kind:'feedback_needed',created_at:'2026-08-30T00:35:00Z',edited_at:'2026-08-30T00:35:00Z',text:'Should this reader preserve the current draft?'},{id:'N3',kind:'regular',created_at:'2026-08-30T00:36:00Z',edited_at:'2026-08-30T00:36:00Z',text:'Editable note with [runbook](/docs/runbook)'}], attachments:[{id:'A1',filename:'proof.png',created_at:'2026-08-30T00:40:00Z'}] };

function normalizedCreatedTicket(body:Record<string,unknown>){const original=(typeof body.title==='string'?body.title:'').trim();let title=original,tags=Array.isArray(body.tags)?body.tags.filter((tag):tag is string=>typeof tag==='string'):[];if(original.startsWith('\\['))title=original.slice(1);else{let rest=original;const found:string[]=[];while(rest.startsWith('[')){const close=rest.indexOf(']'),content=close<0?'':rest.slice(1,close);if(close<0||!content.trim()||content.includes('['))break;found.push(content.trim().replaceAll(/\s+/g,'-'));rest=rest.slice(close+1).trimStart()}if(found.length&&rest){title=rest.trim();tags=[...new Set([...tags,...found])]}}return{title,tags}}

async function mockProject(page: import('@playwright/test').Page, canUpdate = true, primaryFeedbackNeeded = false, ticketLoadDelay = 0, batchResponseDelay = 0, patchResponseDelay = 0) {
  let rows = [{...row,feedback_needed:primaryFeedbackNeeded},backlogRow,archiveRow,deletedRow,movedRow,notStartedRow,completedRow,verifiedRow,startedRow2,startedRow3,searchSlugRow,searchDetailsRow];
  let selectedFull = primaryFeedbackNeeded
    ? {...full,feedback_needed:true,notes:[...full.notes,{id:'N4',kind:'feedback_needed',created_at:'2026-08-30T00:37:00Z',edited_at:'2026-08-30T00:37:00Z',text:'FEEDBACK NEEDED\n\nHello there\n\n1. Something\n2. Another thing'}]}
    : {...full,feedback_needed:false};
  const evidenceByTicket = new Map<string,Array<{id:string;filename:string;created_at:string}>>();
  const patches: Record<string,unknown>[] = [];
  let commandDefinitions=[{id:'check',title:'Run checks',program:'/usr/bin/true',args:[],group:'Quality'}];
  let commandRuns:Array<{id:string;command_id:string;state:'running'|'completed'|'failed'|'cancelled';exit_code?:number;output:Array<{seq:number;stream:string;text:string}>}>=[];
  let createdTerminal=false;
  await page.route('**/*', async route => {
    const request=route.request(), url=new URL(request.url()), path=url.pathname;
    if(path==='/__hotsheet/projects/open') return route.fulfill({status:201,json:project});
    if(path.endsWith('/providers')) return route.fulfill({json:[{connection_id:'git-local',provider:'git',display_name:'Hot Sheet git',locator:'/tickets',default:true,capabilities:{create:true,update:canUpdate,close:true,notes:true,note_edit:canUpdate,note_delete:canUpdate,attachments:true,assignment:true,review_requests:true,dependencies:true,up_next:true,close_reasons:true,claims:true,atomic_batch:true,not_working_report:canUpdate,offline_mutation:true,history:true,watch:true,provider_idempotency:true,query_fields:[]}}]});
    if(path.endsWith('/permissions')&&request.method()==='GET')return route.fulfill({json:[]});
    if(path.endsWith('/connections')&&request.method()==='GET')return route.fulfill({json:[]});
    if(path.endsWith('/commands')&&request.method()==='GET')return route.fulfill({json:commandDefinitions});
    if(path.endsWith('/commands')&&request.method()==='PUT'){commandDefinitions=request.postDataJSON();return route.fulfill({json:commandDefinitions})}
    if(path.endsWith('/command-runs')&&request.method()==='GET')return route.fulfill({json:commandRuns});
    if(path.endsWith('/terminals')&&request.method()==='POST'){createdTerminal=true;return route.fulfill({json:{id:'terminal-new',alive:true,busy:false,cwd:'/work/demo'}})}
    if(path.endsWith('/terminals')&&request.method()==='GET')return route.fulfill({json:[{id:'codex-main',alive:true,busy:true,cwd:'/work/demo',progress:68},{id:'tests',alive:true,busy:false,cwd:'/work/demo'},...(createdTerminal?[{id:'terminal-new',alive:true,busy:false,cwd:'/work/demo'}]:[])]});
    if(path.endsWith('/terminals/codex-main')&&request.method()==='GET')return route.fulfill({json:{id:'codex-main',alive:true,busy:true,cwd:'/work/demo',progress:68,scrollback:'Implementing terminal dashboard\nRunning browser checks…'}});
    if(path.endsWith('/terminals/tests')&&request.method()==='GET')return route.fulfill({json:{id:'tests',alive:true,busy:false,cwd:'/work/demo',scrollback:'Test Files 42 passed\nWaiting for changes.'}});
    if(path.endsWith('/terminals/terminal-new')&&request.method()==='GET')return route.fulfill({json:{id:'terminal-new',alive:true,busy:false,cwd:'/work/demo',scrollback:'New project terminal'}});
    const commandStart=path.match(/\/commands\/([^/]+)\/run$/);
    if(commandStart&&request.method()==='POST'){const run={id:'run-1',command_id:commandStart[1],state:'running' as const,output:[]};commandRuns=[run,...commandRuns];return route.fulfill({status:202,json:run})}
    const commandCancel=path.match(/\/command-runs\/([^/]+)\/cancel$/);
    if(commandCancel&&request.method()==='POST'){const run={...(commandRuns.find(item=>item.id===commandCancel[1])!),state:'cancelled' as const,output:[{seq:1,stream:'stdout',text:'Stopped by user'}]};commandRuns=commandRuns.map(item=>item.id===run.id?run:item);return route.fulfill({json:run})}
    const commandRun=path.match(/\/command-runs\/([^/]+)$/);
    if(commandRun&&request.method()==='GET')return route.fulfill({json:commandRuns.find(item=>item.id===commandRun[1])});
    if(path.endsWith('/ws/poll')&&request.method()==='GET'){await new Promise(resolve=>setTimeout(resolve,1_000));const since=Number(url.searchParams.get('since')??0);return route.fulfill({json:{cursor:since,events:[],overflow:false}})}
    if(path.endsWith('/repository/status')) return route.fulfill({json:{branch:'main',ahead:1,behind:0,staged:0,unstaged:1,untracked:0,conflicted:0,clean:false}});
    if(path.endsWith('/tickets/01/code-review')&&request.method()==='GET')return route.fulfill({json:{difftool:'Glassbox',truncated:false,ranges:[{from:'aaa1111',to:'bbb2222',count:2},{from:'ccc3333',to:'ddd4444',count:2}],commits:[{sha:'ddd4444',short_sha:'ddd4444',subject:'HS2-DEMO01: finish the later review bundle',committed_at:'2026-09-02T10:00:00Z'},{sha:'ccc3333',short_sha:'ccc3333',subject:'HS2-DEMO01: start the later review bundle',committed_at:'2026-09-02T09:00:00Z'},{sha:'bbb2222',short_sha:'bbb2222',subject:'HS2-DEMO01: finish the responsive review segment',committed_at:'2026-09-02T08:00:00Z'},{sha:'aaa1111',short_sha:'aaa1111',subject:'HS2-DEMO01: discover associated commits',committed_at:'2026-09-02T07:00:00Z'}]}});
    if(path.endsWith('/tickets/01/code-review')&&request.method()==='POST'){patches.push({operation:'code-review',...request.postDataJSON()});return route.fulfill({status:204})}
    if(path.endsWith('/corrupt-tickets')&&request.method()==='GET') return route.fulfill({json:[]});
    if(path.endsWith('/batch')&&request.method()==='POST'){const updates=request.postDataJSON().updates as Array<Record<string,unknown>&{id:string}>;const changed=updates.map(({id,...body})=>{patches.push(body);rows=rows.map(item=>item.id===id?{...item,...body}:item);const ticket=rows.find(item=>item.id===id)!;return{store:'git-local',...ticket,details:'',blocked_reason:null,notes:[],attachments:evidenceByTicket.get(id)??[],concurrency_token:`next-${id}`}});if(batchResponseDelay)await new Promise(resolve=>setTimeout(resolve,batchResponseDelay));return route.fulfill({json:changed})}
    if(path.endsWith('/tickets')&&request.method()==='GET'){if(url.searchParams.get('text')==='QQRY00'){await new Promise(resolve=>setTimeout(resolve,150));return route.fulfill({json:[searchSlugRow,searchDetailsRow]})}return route.fulfill({json:rows})}
    if(path.endsWith('/tickets')&&request.method()==='POST'){const body=request.postDataJSON(),normalized=normalizedCreatedTicket(body);const created={...row,id:'02',native_id:'02',slug:'HS2-NEW001',title:normalized.title,tags:normalized.tags,category:body.category,status:body.status??'not_started',up_next:false};rows=[created,...rows];return route.fulfill({status:201,json:{...created,details:'',notes:[],attachments:[]}})}
    if(path.endsWith('/provider-attachments/copy')&&request.method()==='POST'){const destination=rows.find(item=>item.native_id===request.postDataJSON().destination.native_id)!;return route.fulfill({status:201,json:{...destination,details:'',notes:[],attachments:[{id:'A-COPY',filename:'proof.png',created_at:'2026-08-30T01:15:00Z'}]}})}
    if(path.endsWith('/tickets/01')&&request.method()==='GET'){if(ticketLoadDelay)await new Promise(resolve=>setTimeout(resolve,ticketLoadDelay));return route.fulfill({json:{store:'git-local',...selectedFull}})}
    const attachmentMatch=path.match(/\/tickets\/([^/]+)\/attachments$/);
    if(attachmentMatch&&attachmentMatch[1]!=='01'&&request.method()==='POST'){const id=attachmentMatch[1],items=evidenceByTicket.get(id)??[],filename=decodeURIComponent(request.headers()['x-hotsheet-filename']??'attachment'),attachment={id:`E${items.length+1}`,filename,created_at:'2026-08-30T01:10:00Z'},next=[...items,attachment];evidenceByTicket.set(id,next);const ticket=rows.find(item=>item.id===id)!;return route.fulfill({status:201,json:{store:'git-local',...ticket,details:'',blocked_reason:null,notes:[],attachments:next}})}
    const attachmentDeleteMatch=path.match(/\/tickets\/([^/]+)\/attachments\/([^/]+)$/);
    if(attachmentDeleteMatch&&attachmentDeleteMatch[1]!=='01'&&request.method()==='DELETE'){const [,id,attachmentId]=attachmentDeleteMatch,next=(evidenceByTicket.get(id)??[]).filter(item=>item.id!==attachmentId);evidenceByTicket.set(id,next);const ticket=rows.find(item=>item.id===id)!;return route.fulfill({json:{store:'git-local',...ticket,details:'',blocked_reason:null,notes:[],attachments:next}})}
    const notWorkingMatch=path.match(/\/providers\/[^/]+\/tickets\/([^/]+)\/not-working$/);
    if(notWorkingMatch&&request.method()==='POST'){const id=notWorkingMatch[1],raw=request.postData()??'',attachments=raw.includes('filename=')?[{id:'NW1',filename:'proof ünicode.png',created_at:'2026-08-30T01:10:00Z'}]:[];evidenceByTicket.set(id,attachments);rows=rows.map(item=>item.id===id?{...item,status:'not_started',up_next:true}:item);patches.push({operation:'not-working',status:'not_started',up_next:true,raw});const changed=rows.find(item=>item.id===id)!,notes=[{id:'NW-ACTIVITY',kind:'activity',created_at:'2026-08-30T01:10:00Z',edited_at:'2026-08-30T01:10:00Z',text:'Brian reported as not working\nThe fix regressed after restart.'}];return route.fulfill({json:{store:'git-local',...changed,details:'',blocked_reason:null,notes,attachments}})}
    if(path.includes('/tickets/')&&request.method()==='GET'){if(ticketLoadDelay)await new Promise(resolve=>setTimeout(resolve,ticketLoadDelay));const id=path.split('/').pop(),ticket=rows.find(item=>item.id===id);if(ticket)return route.fulfill({json:{store:'git-local',...ticket,details:'',blocked_reason:null,notes:[],attachments:evidenceByTicket.get(id!)??[],concurrency_token:`token-${id}`}})}
    if(path.endsWith('/tickets/01/attachments')&&request.method()==='POST'){const filename=request.headers()['x-hotsheet-filename']??'attachment';selectedFull={...selectedFull,attachments:[...selectedFull.attachments,{id:`A${selectedFull.attachments.length+1}`,filename,created_at:'2026-08-30T01:10:00Z'}]};return route.fulfill({status:201,json:{store:'git-local',...selectedFull}})}
    if(path.includes('/tickets/01/attachments/')&&request.method()==='GET')return route.fulfill({body:'attachment bytes',headers:{'content-type':'application/octet-stream','x-hotsheet-filename':'proof.png'}});
    if(path.includes('/tickets/01/attachments/')&&request.method()==='DELETE'){const attachmentId=path.split('/').pop();selectedFull={...selectedFull,attachments:selectedFull.attachments.filter(item=>item.id!==attachmentId)};return route.fulfill({json:{store:'git-local',...selectedFull}})}
    if(path.includes('/tickets/01/notes/')&&request.method()==='DELETE'){const noteId=path.split('/').pop();selectedFull={...selectedFull,notes:selectedFull.notes.filter(note=>{return note.id!==noteId})};return route.fulfill({json:{store:'git-local',...selectedFull}})}
    if(path.includes('/tickets/')&&request.method()==='PATCH'){const id=path.split('/').pop(),body=request.postDataJSON();patches.push(body);if(body.note_kind==='regular')rows=rows.map(item=>item.id===id?{...item,feedback_needed:false}:item);else rows=rows.map(item=>item.id===id?{...item,...body}:item);if(id==='01'){const label=(value:string)=>value.split('_').map(part=>part.charAt(0).toUpperCase()+part.slice(1)).join(' '),statusNote=typeof body.status==='string'&&body.status!==selectedFull.status?{id:`N-status-${patches.length}`,kind:'activity' as const,created_at:'2026-09-02T02:00:00Z',edited_at:'2026-09-02T02:00:00Z',text:`Status changed from ${label(selectedFull.status)} to ${label(body.status)}`}:undefined,appendedNote=typeof body.note==='string'&&body.note_kind==='regular'?{id:`N-response-${patches.length}`,kind:'regular' as const,created_at:'2026-09-02T02:01:00Z',edited_at:'2026-09-02T02:01:00Z',text:body.note}:undefined;selectedFull={...selectedFull,...body,feedback_needed:appendedNote?false:selectedFull.feedback_needed,notes:[...selectedFull.notes,...statusNote?[statusNote]:[],...appendedNote?[appendedNote]:[]]};if(patchResponseDelay)await new Promise(resolve=>setTimeout(resolve,patchResponseDelay));return route.fulfill({json:{store:'git-local',...selectedFull}})}const changed=rows.find(item=>item.id===id)!;return route.fulfill({json:{store:'git-local',...changed,details:'',notes:[],attachments:evidenceByTicket.get(id!)??[]}})}
    return route.continue();
  });
  return patches;
}

test('activates Dev Review from the main app query only in explicit development review mode',async({page})=>{
  await page.goto('/?dev-review=1');await expect(page.locator('.hs-dev-review')).toBeVisible();await expect(page.getByRole('button',{name:'Feedback'})).toBeVisible();await page.screenshot({path:'/private/tmp/hs2-sv3f5g-main-dev-review-wide.png',fullPage:true});
  await page.setViewportSize({width:390,height:844});await expect(page.locator('.hs-dev-review')).toBeVisible();await page.screenshot({path:'/private/tmp/hs2-sv3f5g-main-dev-review-narrow.png',fullPage:true});
  await page.goto('/?dev-review=0');await expect(page.locator('.hs-dev-review')).toHaveCount(0);
});

test('uses independent width and height terminal dashboard zoom scales',async({page})=>{
  await page.setViewportSize({width:1440,height:1100});await mockProject(page);await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();
  await page.getByRole('button',{name:'Terminal dashboard'}).click();const dashboard=page.getByRole('region',{name:'Terminal dashboard'});await expect(dashboard).toBeVisible();await expect(dashboard).toHaveAttribute('data-basis','across');await expect(dashboard).toHaveAttribute('data-fit','4');await expect(dashboard.getByText('Running browser checks…')).toBeVisible();await expect(dashboard.locator('.terminal-tile__preview').first()).toHaveCSS('background-color','rgb(55, 65, 81)');
  await dashboard.getByRole('button',{name:/Zoom in, fit fewer terminals across/}).click();await expect(dashboard).toHaveAttribute('data-fit','3');await expect(page.evaluate(()=>localStorage.getItem('hotsheet.terminals.fit-across'))).resolves.toBe('3');
  await dashboard.getByRole('button',{name:'Magnify codex-main'}).click();await expect(dashboard.getByRole('dialog',{name:'Magnified codex-main'})).toBeVisible();await page.keyboard.press('Escape');await expect(dashboard.getByRole('dialog')).toHaveCount(0);await page.screenshot({path:'/private/tmp/hs2-2zcn7k-terminal-dashboard-wide.png',fullPage:true});
  await page.setViewportSize({width:1100,height:720});await expect(dashboard).toHaveAttribute('data-basis','high');await expect(dashboard).toHaveAttribute('data-fit','2');await dashboard.getByRole('button',{name:/Zoom out, fit more terminals high/}).click();await expect(dashboard).toHaveAttribute('data-fit','3');await expect(page.evaluate(()=>localStorage.getItem('hotsheet.terminals.fit-high'))).resolves.toBe('3');await expect(page.evaluate(()=>localStorage.getItem('hotsheet.terminals.fit-across'))).resolves.toBe('3');
  await dashboard.getByRole('button',{name:'Hide codex-main'}).click();const showHidden=page.getByRole('button',{name:'Show hidden terminals'});await expect(showHidden).toBeEnabled();await expect(page.locator('.terminal-dashboard-controls__count')).toHaveText('1');await showHidden.click();await expect(dashboard.getByText('Running browser checks…')).toBeVisible();await page.screenshot({path:'/private/tmp/hs2-2zcn7k-terminal-dashboard-short.png',fullPage:true});
  await page.getByRole('tab',{name:/demo/}).click();await expect(page.getByRole('button',{name:'Terminal dashboard'})).toHaveAttribute('aria-pressed','false');
});

test('streams ANSI terminal output, input, viewport leases, driver state, and reconnects without exposing credentials',async({page})=>{
  await page.addInitScript(()=>{
    const sockets:FakeTerminalSocket[]=[];
    class FakeTerminalSocket extends EventTarget {
      static CONNECTING=0;static OPEN=1;static CLOSING=2;static CLOSED=3;readyState=FakeTerminalSocket.CONNECTING;binaryType='blob';sent:unknown[]=[];sizeSent=false;
      constructor(public url:string){super();sockets.push(this);setTimeout(()=>{this.readyState=FakeTerminalSocket.OPEN;this.dispatchEvent(new Event('open'));this.emitBytes('\u001b[32mLive terminal ready\u001b[0m\r\n')})}
      send(value:unknown){this.sent.push(value);if(typeof value!=='string')return;try{const claim=JSON.parse(value) as {resize?:{viewer_id:string}};if(claim.resize){if(!this.sizeSent){this.sizeSent=true;this.dispatchEvent(new MessageEvent('message',{data:JSON.stringify({pty_size:{cols:100,rows:30},driven_by:claim.resize.viewer_id})}))}return}}catch{/* terminal input */}this.emitBytes(value)}
      close(){if(this.readyState===FakeTerminalSocket.CLOSED)return;this.readyState=FakeTerminalSocket.CLOSED;this.dispatchEvent(new CloseEvent('close'))}
      emitBytes(value:string){this.dispatchEvent(new MessageEvent('message',{data:new TextEncoder().encode(value).buffer}))}
      emitSize(cols:number,rows:number,drivenBy:string){this.dispatchEvent(new MessageEvent('message',{data:JSON.stringify({pty_size:{cols,rows},driven_by:drivenBy})}))}
    }
    Object.assign(window,{WebSocket:FakeTerminalSocket,__terminalSockets:sockets});
  });
  await mockProject(page);await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();await page.getByRole('button',{name:'Terminal dashboard'}).click();
  const viewport=page.locator('[data-component="terminal-viewport"][data-terminal-id="codex-main"]').first();await expect(viewport).toHaveAttribute('data-connection','connected');await expect(viewport.locator('.xterm-rows')).toContainText('Live terminal ready');await expect(viewport).toHaveAttribute('data-pty-size','100x30');await expect(viewport).toHaveAttribute('data-driving','true');
  await viewport.locator('.xterm-helper-textarea').focus();await page.keyboard.type('browser input');await expect(viewport.locator('.xterm-rows')).toContainText('browser input');
  const protocol=await page.evaluate(()=>{const sockets=(window as unknown as {__terminalSockets:Array<{url:string;sent:unknown[]}>}).__terminalSockets.filter(socket=>socket.sent.some(value=>typeof value==='string'&&value.includes('viewer_id')));return{urls:sockets.map(socket=>socket.url),claims:sockets.flatMap(socket=>socket.sent.filter((value):value is string=>typeof value==='string'&&value.includes('viewer_id')).map(value=>JSON.parse(value)))}});expect(protocol.urls.every(url=>url.includes('/__hotsheet/project-api/')&&!url.includes('secret'))).toBe(true);expect(protocol.claims[0].resize).toMatchObject({cols:expect.any(Number),rows:expect.any(Number),focus:expect.any(Boolean),visible:true});
  await page.getByRole('button',{name:'Magnify codex-main'}).click();const magnified=page.getByRole('dialog',{name:'Magnified codex-main'}).locator('[data-component="terminal-viewport"]');await expect(magnified).toHaveAttribute('data-connection','connected');const viewers=await page.evaluate(()=>{const sockets=(window as unknown as {__terminalSockets:Array<{sent:unknown[]}>}).__terminalSockets.filter(socket=>socket.sent.some(value=>typeof value==='string'&&value.includes('viewer_id')));return sockets.map(socket=>JSON.parse(socket.sent.find(value=>typeof value==='string'&&value.includes('viewer_id')) as string).resize.viewer_id)});expect(new Set(viewers).size).toBe(viewers.length);
  await page.evaluate(()=>{const sockets=(window as unknown as {__terminalSockets:Array<{sent:unknown[];emitSize(cols:number,rows:number,driver:string):void}>}).__terminalSockets.filter(socket=>socket.sent.some(value=>typeof value==='string'&&value.includes('viewer_id'))),driver=JSON.parse(sockets[0].sent.find(value=>typeof value==='string'&&value.includes('viewer_id')) as string).resize.viewer_id;sockets.at(-1)!.emitSize(120,40,driver)});await expect(magnified).toHaveAttribute('data-driving','false');await expect(magnified).toHaveAttribute('data-pty-size','120x40');await expect(magnified).toHaveAttribute('data-viewing-label','Viewing at 120×40 · focus to resize');expect(Number(await magnified.getAttribute('data-scale'))).toBeLessThan(1);expect(Number(await magnified.getAttribute('data-scale'))).toBeGreaterThanOrEqual(.7);await page.screenshot({path:'/private/tmp/hs2-pd4mz9-live-terminal.png',fullPage:true});
  const before=await page.evaluate(()=>(window as unknown as {__terminalSockets:unknown[]}).__terminalSockets.length);await page.evaluate(()=> { (window as unknown as {__terminalSockets:Array<{sent:unknown[];close():void}>}).__terminalSockets.find(socket=>socket.sent.some(value=>typeof value==='string'&&value.includes('viewer_id')))!.close(); });await expect.poll(()=>page.evaluate(()=>(window as unknown as {__terminalSockets:unknown[]}).__terminalSockets.length)).toBeGreaterThan(before);
});

test('opens, navigates, resizes, zooms, creates, hides, and restores the project terminal drawer',async({page})=>{
  await page.setViewportSize({width:1440,height:900});await mockProject(page);await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();await expect(page.getByRole('button',{name:'Show terminal drawer'})).toBeVisible();
  await page.getByRole('button',{name:'Terminal dashboard'}).click();const globalTile=page.locator('[data-component="terminal-tile"][data-terminal-key="demo-checkout:codex-main"]');await globalTile.hover();await globalTile.getByRole('button',{name:'Open codex-main in demo'}).click();const drawer=page.locator('[data-component="terminal-drawer"]');await expect(drawer).toBeVisible();await expect(drawer).toHaveAttribute('data-mode','dedicated');await expect(drawer.getByRole('tab',{name:/codex-main/})).toHaveAttribute('aria-selected','true');
  const handle=page.getByRole('separator',{name:'Resize Terminal drawer'});await expect(handle).toHaveAttribute('aria-valuenow','320');await handle.focus();await page.keyboard.press('ArrowUp');await expect(handle).toHaveAttribute('aria-valuenow','336');await expect(page.evaluate(()=>localStorage.getItem('hotsheet.layout.app-terminal-drawer.size'))).resolves.toBe('336');
  await drawer.getByRole('tab',{name:'Terminal grid'}).click();await expect(drawer).toHaveAttribute('data-mode','grid');const dashboard=drawer.getByRole('region',{name:'Terminal dashboard'});await expect(dashboard).toHaveAttribute('data-basis','high');await expect(dashboard).toHaveAttribute('data-fit','2');await dashboard.getByRole('button',{name:/Zoom out/}).click();await expect(dashboard).toHaveAttribute('data-fit','3');await expect(page.evaluate(()=>localStorage.getItem('hotsheet.terminals.drawer-fit-high'))).resolves.toBe('3');
  const codexTile=drawer.locator('[data-component="terminal-tile"][data-terminal-key="demo-checkout:codex-main"]');await codexTile.hover();await codexTile.getByRole('button',{name:'Hide codex-main'}).click();await expect(codexTile).toHaveCount(0);await drawer.getByRole('button',{name:'Show 1 hidden project terminal'}).click();await expect(codexTile).toHaveCount(1);
  await drawer.getByRole('button',{name:'New project terminal'}).click();await expect(drawer.getByRole('tab',{name:/terminal-new/})).toHaveAttribute('aria-selected','true');await expect(drawer).toHaveAttribute('data-mode','dedicated');await page.screenshot({path:'/private/tmp/hs2-586bvq-terminal-drawer-wide.png',fullPage:true});
  await drawer.getByRole('button',{name:'Hide terminal drawer'}).click();await expect(drawer).toHaveCount(0);await expect(page.getByRole('button',{name:'Show terminal drawer'})).toBeVisible();await page.screenshot({path:'/private/tmp/hs2-586bvq-terminal-drawer-collapsed.png',fullPage:true});
  await page.getByRole('button',{name:'Show terminal drawer'}).click();await page.setViewportSize({width:1024,height:600});await expect(page.locator('[data-component="terminal-drawer"]')).toBeVisible();await page.screenshot({path:'/private/tmp/hs2-586bvq-terminal-drawer-short.png',fullPage:true});
});

test('keeps feedback rectangle input within its frame budget in the populated main app',async({page},testInfo)=>{
  await page.setViewportSize({width:1280,height:900});await mockProject(page);await page.goto('/?dev-review=1');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();
  const measurement=await measureFeedbackRectangle(page,{x:440,y:220},{x:820,y:520});await testInfo.attach('feedback-performance.json',{body:JSON.stringify(measurement,null,2),contentType:'application/json'});expectResponsiveFeedbackRectangle(measurement);
  await page.screenshot({path:'/private/tmp/hs2-6ppvjc-main-wide.png',fullPage:true});await page.setViewportSize({width:390,height:844});await page.screenshot({path:'/private/tmp/hs2-6ppvjc-main-narrow.png',fullPage:true});
});

test('projects an indexed feedback-needed note into the real row and inspector rails',async({page})=>{
  await page.setViewportSize({width:1280,height:900});
  await mockProject(page,true,true);await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();
  const ticket=page.locator('[data-ticket-slug="HS2-DEMO01"]');
  await expect(ticket.locator('.ticket-list-row__indicator--needs-review')).toHaveCSS('background-color','rgb(139, 92, 246)');
  await expect(ticket.locator('.ticket-list-row__feedback')).toContainText('Needs review');
  await ticket.click();
  const inspector=page.locator('[data-component="ticket-inspector"]');
  await expect(inspector).toHaveAttribute('data-needs-review','true');
  await expect(inspector.locator('.ticket-inspector__feedback')).toContainText('Needs review');
  await page.screenshot({path:'/private/tmp/hs2-vwphrd-feedback-needed-wide.png',fullPage:true});
  await page.setViewportSize({width:760,height:900});
  await expect(ticket.locator('.ticket-list-row__indicator--needs-review')).toHaveCSS('background-color','rgb(139, 92, 246)');
  await expect(inspector).toHaveAttribute('data-needs-review','true');
  await page.screenshot({path:'/private/tmp/hs2-vwphrd-feedback-needed-narrow.png',fullPage:true});
});

test('clicks exact feedback character positions, removes a split, and composes an interleaved Markdown reply',async({page})=>{
  const patches=await mockProject(page,true,true);await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();await page.locator('[data-ticket-slug="HS2-DEMO01"]').click();await page.getByRole('button',{name:'Open ticket reader'}).click();
  const note=page.getByRole('dialog').locator('article[data-note-id="N4"]'),clickAfter=async(text:string)=>{const block=note.locator('.note-card__feedback-block').filter({hasText:text});await block.scrollIntoViewIfNeeded();const point=await block.evaluate((element,needle)=>{const walker=document.createTreeWalker(element,NodeFilter.SHOW_TEXT);let node:Node|null;while((node=walker.nextNode())){const index=node.textContent?.indexOf(needle)??-1;if(index<0)continue;const range=document.createRange();range.setStart(node,index+needle.length-1);range.setEnd(node,index+needle.length);const rect=range.getBoundingClientRect();return{x:rect.right-.5,y:rect.y+rect.height/2}}throw new Error(`Text not found: ${needle}`)},text);await page.mouse.click(point.x,point.y)};
  expect(await note.getByRole('textbox',{name:'Feedback response'}).evaluate(node=>node.getBoundingClientRect().height)).toBeLessThan(75);
  await expect(note.getByRole('button',{name:'Add response at a character position'})).toHaveCount(1);
  await clickAfter('Something');const first=note.getByRole('textbox',{name:/Response at character/});await expect(first).toBeFocused();await first.fill('Discard me');await note.getByRole('button',{name:/Remove response at character/}).click();await expect(note.locator('.note-card__inline-reply')).toHaveCount(0);
  await clickAfter('Something');await note.getByRole('textbox',{name:/Response at character/}).fill('My first response');
  await clickAfter('Another thing');const second=note.getByRole('textbox',{name:/Response at character/}).last();await expect(second).toBeFocused();await second.fill('My second response');
  await note.screenshot({path:'/private/tmp/hs2-c5sab3-inline-feedback-replies.png'});await page.setViewportSize({width:760,height:900});const noteBox=(await note.boundingBox())!,replyBox=(await second.boundingBox())!;expect(replyBox.x).toBeGreaterThanOrEqual(noteBox.x);expect(replyBox.x+replyBox.width).toBeLessThanOrEqual(noteBox.x+noteBox.width);await note.screenshot({path:'/private/tmp/hs2-c5sab3-inline-feedback-replies-narrow.png'});await note.getByRole('button',{name:'Respond'}).click();
  await expect.poll(()=>patches.find(patch=>patch.note_kind==='regular')?.note).toBe('> FEEDBACK NEEDED\n>\n> Hello there\n>\n> 1. Something\n\nMy first response\n\n> 2. Another thing\n\nMy second response');
});

test('records No response needed as a subtle regular response and clears review state',async({page})=>{
  const patches=await mockProject(page,true,true);await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();const ticket=page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-DEMO01"]');await ticket.click();await page.getByRole('button',{name:'Open ticket reader'}).click();const reader=page.getByRole('dialog');await reader.getByRole('button',{name:'No response needed'}).click();await expect.poll(()=>patches.some(patch=>patch.note==='No response needed'&&patch.note_kind==='regular')).toBe(true);await expect(reader.locator('[data-acknowledgement="true"]')).toContainText('No response needed');await page.getByRole('button',{name:'Close ticket reader'}).click();await expect(ticket.locator('.ticket-list-row__feedback')).toHaveCount(0);await ticket.click();const acknowledgement=page.locator('[data-component="ticket-inspector"] [data-acknowledgement="true"]');await expect(acknowledgement).toBeVisible();await acknowledgement.screenshot({path:'/private/tmp/hs2-yk27gp-no-response-needed.png'});
});

test('opens and refreshes repository clean, dirty, ahead, behind, and error states',async({page})=>{
  await mockProject(page);let response={branch:'main',upstream:'origin/main',ahead:0,behind:0,staged:0,unstaged:0,untracked:0,conflicted:0},failed=false;
  await page.route('**/repository/status',route=>failed?route.fulfill({status:500,body:'git unavailable'}):route.fulfill({json:response}));
  await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();
  const summary=page.locator('[data-component="repository-summary"]');await expect(summary).toHaveAttribute('data-state','clean');await summary.getByRole('button').click();const popover=page.locator('[data-component="repository-status-popover"]');await expect(popover).toHaveAttribute('data-state','clean');
  response={...response,ahead:2,behind:3,staged:1,unstaged:2,untracked:4};await popover.getByRole('button',{name:'Refresh'}).click();await expect(popover).toHaveAttribute('data-state','diverged');await expect(popover).toContainText('2 ahead');await expect(popover).toContainText('3 behind');await expect(summary).toHaveAttribute('data-state','dirty');await popover.screenshot({path:'/private/tmp/hs2-rpvfa4-repository-status.png'});
  failed=true;await popover.getByRole('button',{name:'Refresh'}).click();await expect(popover).toHaveAttribute('data-state','error');await expect(popover.getByRole('alert')).toBeVisible();await expect(summary).toHaveAttribute('data-state','error');
});

test('keeps exactly 24px above and below the nearly full-height ticket reader',async({page})=>{
  await page.setViewportSize({width:1280,height:900});await mockProject(page);await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();await page.locator('[data-ticket-slug="HS2-DEMO01"]').click();await page.getByRole('button',{name:'Open ticket reader'}).click();
  const reader=page.getByRole('dialog',{name:'Read and edit HS2-DEMO01'}),wide=await reader.boundingBox();expect(wide!.y).toBe(24);expect(wide!.height).toBe(852);await page.screenshot({path:'/private/tmp/hs2-h5vjet-reader-height-wide.png',fullPage:true});
  await page.setViewportSize({width:760,height:900});const narrow=await reader.boundingBox();expect(narrow!.y).toBe(24);expect(narrow!.height).toBe(852);await page.screenshot({path:'/private/tmp/hs2-h5vjet-reader-height-narrow.png',fullPage:true});
});

test('clears needs review when a regular response follows the feedback-needed note',async({page})=>{
  await mockProject(page);await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();
  const ticket=page.locator('[data-ticket-slug="HS2-DEMO01"]');
  await expect(ticket.locator('.ticket-list-row__indicator--needs-review')).toHaveCount(0);
  await expect(ticket.locator('.ticket-list-row__feedback')).toHaveCount(0);
  await ticket.click();
  const inspector=page.locator('[data-component="ticket-inspector"]');
  await expect(inspector).toHaveAttribute('data-needs-review','false');
  await expect(inspector.locator('.ticket-inspector__feedback')).toHaveCount(0);
  await expect(inspector.locator('[data-component="note-card"][data-note-id="N2"]')).toHaveAttribute('data-kind','regular');
  await inspector.locator('[data-component="note-card"][data-note-id="N2"]').screenshot({path:'/private/tmp/hs2-98q45q-answered-feedback.png'});
});

test('shows Blocked only while a dependency remains unresolved',async({page})=>{
  await mockProject(page);const target={...row,blocked_by:[completedRow.id]};await page.route('**/tickets',route=>route.request().method()==='GET'?route.fulfill({json:[target,completedRow]}):route.fallback());await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();await expect(page.locator('[data-project-dialog]')).toBeHidden();const ticket=page.locator('[data-ticket-slug="HS2-DEMO01"]');await expect(ticket.locator('[data-component="blocked-badge"]')).toHaveCount(0);await ticket.screenshot({path:'/private/tmp/hs2-he87en-resolved-blocker.png'});
  await page.route('**/tickets',route=>route.request().method()==='GET'?route.fulfill({json:[{...target,blocked_by:[notStartedRow.id]},notStartedRow]}):route.fallback());await page.reload();await expect(ticket.locator('[data-component="blocked-badge"]')).toHaveText('Blocked');await ticket.screenshot({path:'/private/tmp/hs2-he87en-unresolved-blocker.png'});
});

test('lists associated commits and opens a validated commit or range in the configured diff tool',async({page})=>{
  const actions=await mockProject(page);await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();await page.locator('[data-ticket-slug="HS2-DEMO01"]').click();
  const inspector=page.locator('[data-component="ticket-inspector"]');await inspector.getByRole('button',{name:'Code Review'}).click();const review=inspector.locator('[data-component="ticket-code-review"]');
  const commits=review.locator('.ticket-code-review__commits'),firstCommit=commits.locator(':scope > li').first();const expectFlushCommit=async()=>{expect(Math.abs(await firstCommit.evaluate(node=>node.getBoundingClientRect().left)-await commits.evaluate(node=>node.getBoundingClientRect().left))).toBeLessThanOrEqual(1)};
  await expect(inspector.getByRole('button',{name:'Code Review'}).locator('[data-lucide="message-square-code"]')).toBeVisible();await expect(review).toContainText('Opens in Glassbox');await expect(review.locator('[data-commit-sha]')).toHaveCount(4);await expect(review.locator('.ticket-code-review__range')).toHaveCount(2);await expect(review).toContainText('HS2-DEMO01: finish the responsive review segment');await expectFlushCommit();
  await review.getByRole('button',{name:'Open 2 commit bundle aaa1111 through bbb2222 in Glassbox'}).click();await expect.poll(()=>actions.some(action=>action.operation==='code-review'&&action.mode==='range'&&action.from==='aaa1111'&&action.to==='bbb2222')).toBe(true);await review.getByRole('button',{name:'Open 2 commit bundle ccc3333 through ddd4444 in Glassbox'}).click();await expect.poll(()=>actions.some(action=>action.operation==='code-review'&&action.mode==='range'&&action.from==='ccc3333'&&action.to==='ddd4444')).toBe(true);
  await review.getByRole('button',{name:'Open commit bbb2222 in Glassbox'}).click();await expect.poll(()=>actions.some(action=>action.operation==='code-review'&&action.mode==='commit'&&action.commit==='bbb2222')).toBe(true);
  await page.screenshot({path:'/private/tmp/hs2-ggjed1-code-review-wide.png',fullPage:true});
  await page.setViewportSize({width:1024,height:600});await expect(review.locator('.ticket-code-review__range')).toHaveCount(2);await expectFlushCommit();await page.screenshot({path:'/private/tmp/hs2-ggjed1-code-review-floor.png',fullPage:true});
});

test('shows active work only for the lifetime of a live ticket claim lease',async({page})=>{
  await mockProject(page);const expires=new Date(Date.now()+5_000).toISOString(),active={...row,claimed_by:'codex-worker',worker_label:'Codex',claim_lease_expires_at:expires,claim_count:1},previouslyClaimed={...startedRow2,claim_count:4};
  await page.route('**/tickets',route=>route.request().method()==='GET'?route.fulfill({json:[active,previouslyClaimed]}):route.fallback());
  await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();await expect(page.locator('[data-project-dialog]')).toBeHidden();await page.waitForTimeout(500);
  const activeRow=page.locator('[data-ticket-slug="HS2-DEMO01"]'),idleRow=page.locator('[data-ticket-slug="HS2-START02"]'),indicator=activeRow.locator('.ticket-list-row__active-work'),workSummary=page.locator('[data-component="project-work-summary"]');
  await expect(workSummary).toContainText('1 active');
  await expect(indicator).toHaveAttribute('aria-label','Codex actively working');await expect(idleRow.locator('.ticket-list-row__active-work')).toHaveCount(0);expect(await activeRow.locator('.ticket-list-row__metadata').evaluate(node=>[...node.children].map(child=>child.className))).toEqual(expect.arrayContaining(['ticket-list-row__active-work']));
  await page.screenshot({path:'/private/tmp/hs2-w77014-active-work-wide.png',fullPage:true});await page.setViewportSize({width:390,height:844});await expect(indicator).toBeVisible();await page.screenshot({path:'/private/tmp/hs2-w77014-active-work-narrow.png',fullPage:true});
  await expect(indicator).toHaveCount(0,{timeout:6_000});await expect(workSummary).toContainText('0 active');
});

test('keeps the visible inspector region mounted while a selected ticket loads',async({page,context})=>{
  await context.grantPermissions(['clipboard-read','clipboard-write']);
  await mockProject(page,true,false,300);await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();
  const region=page.locator('[data-component="resizable-region"][data-region-id="app-inspector"]');await expect(region).toBeVisible();const before=await region.evaluate(node=>node.getBoundingClientRect().width);
  await page.locator('[data-ticket-slug="HS2-DEMO01"]').click();await page.waitForTimeout(75);
  await expect(region).toBeVisible();await expect(region.locator('.ticket-inspector-placeholder')).toBeVisible();await expect(region.locator('.ticket-inspector-placeholder > .toolbar')).toHaveAttribute('data-divider','false');expect(await region.evaluate(node=>node.getBoundingClientRect().width)).toBe(before);
  await expect(region.locator('[data-component="ticket-inspector"]')).toBeVisible();
  await region.getByRole('button',{name:'Copy ticket number HS2-DEMO01'}).click();await expect(page.getByText('HS2-DEMO01 copied to clipboard.',{exact:true})).toBeVisible();await expect.poll(()=>page.evaluate(()=>navigator.clipboard.readText())).toBe('HS2-DEMO01');
  await page.setViewportSize({width:1024,height:600});await page.locator('[data-ticket-slug="HS2-START02"]').click();await page.waitForTimeout(75);await expect(region.locator('.ticket-inspector-transition')).toHaveAttribute('aria-busy','true');await expect(region.locator('.ticket-inspector-placeholder')).toHaveCount(0);await expect(region).toContainText('Use real project tickets');await page.screenshot({path:'/private/tmp/hs2-zt5qnw-inspector-transition-floor.png',fullPage:true});await expect(region.locator('[data-component="ticket-inspector"][data-ticket-slug="HS2-START02"]')).toBeVisible();
});

test('keeps an active editor stable when its already-selected ticket is clicked again',async({page})=>{
  const patches=await mockProject(page);let detailReads=0;page.on('request',request=>{if(request.method()==='GET'&&new URL(request.url()).pathname.endsWith('/tickets/01'))detailReads+=1});
  await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();const row=page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-DEMO01"]');await row.click();
  const inspector=page.locator('[data-component="ticket-inspector"]');await inspector.getByRole('button',{name:'Edit Ticket details'}).dblclick();const editor=inspector.getByRole('textbox',{name:'Ticket details'});await editor.fill('Draft preserved across a redundant reselect');await expect.poll(()=>patches.some(patch=>patch.details==='Draft preserved across a redundant reselect')).toBe(true);await page.waitForTimeout(100);
  const readsBefore=detailReads;await editor.evaluate(node=>{(node as HTMLElement&{reselectionMarker?:boolean}).reselectionMarker=true});await resetRenderMetrics(page);await row.click();
  await expect(editor).toBeFocused();await expect(editor).toHaveValue('Draft preserved across a redundant reselect');expect(await editor.evaluate(node=>(node as HTMLElement&{reselectionMarker?:boolean}).reselectionMarker)).toBe(true);expect(detailReads).toBe(readsBefore);expect(await renderMetrics(page)).toEqual({passes:0,mutations:0});
  await page.screenshot({path:'/private/tmp/hs2-e0mjm8-reselect-editor-wide.png',fullPage:true});await page.setViewportSize({width:940,height:844});await expect(editor).toBeFocused();await page.screenshot({path:'/private/tmp/hs2-e0mjm8-reselect-editor-narrow.png',fullPage:true});
});

test('omits separators below every right-sidebar toolbar state',async({page})=>{
  await mockProject(page);await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();await expect(page.locator('[data-project-dialog]')).toBeHidden();await page.waitForTimeout(500);
  const placeholder=page.locator('.ticket-inspector-placeholder'),toolbar=placeholder.locator(':scope > .toolbar');await expect(placeholder).toContainText('Select a ticket to see and edit its details');await expect(toolbar).toHaveAttribute('data-divider','false');await expect(toolbar).toHaveCSS('border-bottom-color','rgba(0, 0, 0, 0)');await page.screenshot({path:'/private/tmp/hs2-gvk7zy-empty-inspector-wide.png',fullPage:true});
  await page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-DEMO01"]').click();await expect(page.locator('[data-component="ticket-inspector"] .ticket-inspector__header > .toolbar')).toHaveAttribute('data-divider','false');await page.getByRole('button',{name:/Notifications view/}).click();const notificationToolbar=page.getByRole('complementary',{name:'Notification inspector'}).locator(':scope > .toolbar');await expect(notificationToolbar).toHaveAttribute('data-divider','false');await page.setViewportSize({width:1024,height:600});await expect(notificationToolbar).toHaveCSS('border-bottom-color','rgba(0, 0, 0, 0)');await page.screenshot({path:'/private/tmp/hs2-f3nk91-right-sidebar-floor.png',fullPage:true});
});

test('resizes and persists both production shell sidebars',async({page})=>{
  await mockProject(page);await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();
  const sidebar=page.locator('[data-component="resizable-region"][data-region-id="app-sidebar"]'),inspector=page.locator('[data-component="resizable-region"][data-region-id="app-inspector"]');
  const sidebarHandle=sidebar.getByRole('separator',{name:'Resize Project sidebar'}),inspectorHandle=inspector.getByRole('separator',{name:'Resize Ticket inspector'});
  const initialSidebar=await sidebar.evaluate(node=>node.getBoundingClientRect().width),initialInspector=await inspector.evaluate(node=>node.getBoundingClientRect().width);
  const sidebarBox=(await sidebarHandle.boundingBox())!,dragPoint={x:sidebarBox.x+sidebarBox.width/2,y:sidebarBox.y+300};await expect.poll(()=>page.evaluate(point=>(document.elementFromPoint(point.x,point.y) as HTMLElement|null)?.closest<HTMLElement>('[data-action]')?.dataset.action,dragPoint)).toBe('resize-region');await page.mouse.move(dragPoint.x,dragPoint.y);await page.mouse.down();await page.mouse.move(dragPoint.x+32,dragPoint.y);await page.mouse.up();
  await expect.poll(()=>sidebar.evaluate(node=>node.getBoundingClientRect().width)).toBe(initialSidebar+32);
  await inspectorHandle.press('ArrowLeft');
  await expect.poll(()=>inspector.evaluate(node=>node.getBoundingClientRect().width)).toBe(initialInspector+16);
  await page.getByLabel('Columns view').click();
  const sortSelect=page.locator('wa-select[name="workspace-sort"]');await sortSelect.click();await sortSelect.locator('wa-option[value="priority"]').click();
  const commandGroup=page.getByRole('button',{name:'Project commands'});await commandGroup.click();await expect(commandGroup).toHaveAttribute('aria-expanded','false');
  await page.screenshot({path:'/private/tmp/hs2-qgg6pf-resizable-sidebars.png',fullPage:true});
  await page.getByRole('button',{name:'Hide project sidebar'}).click();await page.getByRole('button',{name:'Hide ticket inspector'}).click();
  await page.reload();
  await expect(page.getByLabel('Columns view')).toHaveAttribute('aria-pressed','true');await expect(sortSelect).toHaveJSProperty('value','priority');await expect(sortSelect).toHaveAttribute('aria-label','Sort tickets: Priority, ascending');
  await expect(page.getByRole('button',{name:'Show project sidebar'})).toBeVisible();await expect(page.getByRole('button',{name:'Show ticket inspector'})).toBeVisible();
  await page.getByRole('button',{name:'Show project sidebar'}).click();await expect.poll(()=>sidebar.evaluate(node=>node.getBoundingClientRect().width)).toBe(initialSidebar+32);await expect(page.getByRole('button',{name:'Project commands'})).toHaveAttribute('aria-expanded','false');
  await page.getByRole('button',{name:'Show ticket inspector'}).click();await expect.poll(()=>inspector.evaluate(node=>node.getBoundingClientRect().width)).toBe(initialInspector+16);
});

test('uses labels only when the inspector segmented control has enough room',async({page})=>{
  await mockProject(page);await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();await page.locator('[data-ticket-slug="HS2-DEMO01"]').click();
  const sidebarTabs=page.locator('[data-region-id="app-inspector"] .ticket-inspector__tabs');
  expect(await sidebarTabs.locator('.ticket-inspector__tab-label').evaluateAll(labels=>labels.every(label=>getComputedStyle(label).display==='none'))).toBe(true);
  await page.getByRole('button',{name:'Open ticket reader'}).click();const reader=page.getByRole('dialog',{name:'Read and edit HS2-DEMO01'}),readerTabs=reader.locator('.ticket-inspector__tabs');
  await expect(readerTabs.locator('.ticket-inspector__tab-label').first()).toBeVisible();
  await page.screenshot({path:'/private/tmp/hs2-2p9k4y-segments-wide.png',fullPage:true});
  await page.setViewportSize({width:868,height:700});
  expect(await readerTabs.locator('.ticket-inspector__tab-label').evaluateAll(labels=>labels.every(label=>getComputedStyle(label).display==='none'))).toBe(true);
  await expect(readerTabs.getByRole('button',{name:'Code Review'}).locator('svg')).toBeVisible();
  await page.screenshot({path:'/private/tmp/hs2-2p9k4y-segments-narrow.png',fullPage:true});
});

interface RenderMetricsSnapshot { passes:number;mutations:number }
type InstrumentedWindow=typeof window&{__hotsheetRenderMetrics?:{reset():void;snapshot():RenderMetricsSnapshot}};
const resetRenderMetrics=(page:import('@playwright/test').Page)=>page.evaluate(()=>{(window as InstrumentedWindow).__hotsheetRenderMetrics?.reset()});
const renderMetrics=(page:import('@playwright/test').Page)=>page.evaluate(()=>(window as InstrumentedWindow).__hotsheetRenderMetrics?.snapshot());

test('makes no repeated permission requests or renders while an open project is idle',async({page})=>{
  const permissionRequests:string[]=[];page.on('request',request=>{const path=new URL(request.url()).pathname;if(path.endsWith('/permissions')||path.endsWith('/connections'))permissionRequests.push(path)});
  await mockProject(page);await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();
  await page.waitForTimeout(1_000);permissionRequests.length=0;await resetRenderMetrics(page);await page.waitForTimeout(1_700);
  expect(permissionRequests).toEqual([]);
  expect(await renderMetrics(page)).toEqual({passes:0,mutations:0});
});

test('defers ticket refresh without hiding an open select popup',async({page})=>{
  await mockProject(page);let rows=[row,notStartedRow],cursor=0;const polls:Array<import('@playwright/test').Route>=[];
  await page.route('**/tickets',route=>route.request().method()==='GET'?route.fulfill({json:rows}):route.fallback());
  await page.route('**/ws/poll*',route=>{const since=new URL(route.request().url()).searchParams.get('since');if(since===null)return route.fulfill({json:{cursor,events:[],overflow:false}});polls.push(route)});
  await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();await page.locator('[data-ticket-slug="HS2-DEMO01"]').click();
  const select=page.locator('wa-select[name="inspector-status"]');await select.click();await expect.poll(()=>select.evaluate(node=>(node as HTMLElement&{open?:boolean}).open)).toBe(true);await resetRenderMetrics(page);
  const incoming={...startedRow2,slug:'HS2-INCOMING',title:'Incoming while choosing'};rows=[...rows,incoming];await expect.poll(()=>polls.length).toBeGreaterThan(0);cursor+=1;await polls.shift()!.fulfill({json:{cursor,events:[{store:'git-local',kind:'created',id:incoming.id,slug:incoming.slug}],overflow:false}});
  await page.waitForTimeout(300);await expect.poll(()=>select.evaluate(node=>(node as HTMLElement&{open?:boolean}).open)).toBe(true);await expect(page.locator('[data-ticket-slug="HS2-INCOMING"]')).toHaveCount(0);expect(await renderMetrics(page)).toEqual({passes:0,mutations:0});
  await page.keyboard.press('Escape');await expect.poll(()=>select.evaluate(node=>(node as HTMLElement&{open?:boolean}).open)).toBe(false);await expect(page.locator('[data-ticket-slug="HS2-INCOMING"]')).toBeVisible();
});

test('runs grouped local commands, confirms stop, exposes history, and saves settings',async({page})=>{
  await mockProject(page);await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();
  const command=page.getByRole('button',{name:'Run checks'});await expect(page.getByText('Quality',{exact:true})).toBeVisible();await expect(command).toHaveAttribute('title','Press and hold for command history.');
  await command.click();await expect(page.getByRole('button',{name:'Running Run checks'})).toBeVisible();
  await page.getByRole('button',{name:'Running Run checks'}).click();const stop=page.locator('[data-component="command-cancellation-dialog"]');await expect(stop).toBeVisible();await stop.getByRole('button',{name:'Stop command'}).click();await expect(page.getByRole('button',{name:'Run checks'})).toHaveAttribute('title',/Last run: cancelled/);
  await page.getByRole('button',{name:'Run checks'}).dispatchEvent('pointerdown');await page.waitForTimeout(600);await page.getByRole('button',{name:'Run checks'}).dispatchEvent('pointerup');const history=page.locator('[data-component="command-run-dialog"]');await expect(history).toContainText('Stopped by user');await page.screenshot({path:'/private/tmp/hs2-jn3x4w-commands-wide.png',fullPage:true});await history.getByRole('button',{name:'Close'}).click();await page.setViewportSize({width:390,height:844});const hiddenNarrowCommand=page.locator('[data-action="run-command"]');await hiddenNarrowCommand.dispatchEvent('pointerdown');await page.waitForTimeout(600);await hiddenNarrowCommand.dispatchEvent('pointerup');await expect(history).toContainText('Stopped by user');await page.screenshot({path:'/private/tmp/hs2-jn3x4w-commands-narrow.png',fullPage:true});await history.getByRole('button',{name:'Close'}).click();await page.setViewportSize({width:1280,height:720});
  await page.getByLabel('Settings view').click();await page.getByRole('button',{name:'Commands',exact:true}).click();const editor=page.locator('[name="command-settings"]');await editor.fill('[{"id":"review","title":"Review","program":"/usr/bin/true","args":[],"group":"AI"}]');await page.getByRole('button',{name:'Save commands'}).click();await expect(page.getByRole('status')).toContainText('Saved locally.');await page.getByLabel('List view').click();await expect(page.getByRole('button',{name:'Review'})).toBeVisible();
});

test('switches settings categories from the project sidebar',async({page})=>{
  await mockProject(page);await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();
  const selected=page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-DEMO01"]');await selected.click();await expect(selected).toHaveAttribute('data-selected','true');await page.getByLabel('Settings view').click();const navigation=page.getByRole('complementary',{name:'Settings categories'}),placeholder=page.locator('.ticket-inspector-placeholder'),placeholderToolbar=placeholder.locator(':scope > .toolbar');await expect(navigation).toBeVisible();await expect(page.getByRole('heading',{name:'Ticket sources'})).toBeVisible();await expect(page.locator('.project-settings > h2')).toHaveCount(0);await expect(page.locator('[data-component="project-sidebar"]')).toHaveCount(0);await expect(page.getByRole('complementary',{name:'Ticket inspector'})).toBeVisible();await expect(placeholder).toContainText('Select a ticket to see and edit its details');await expect(placeholderToolbar).toHaveAttribute('data-divider','false');await expect(placeholderToolbar).toHaveCSS('border-bottom-color','rgba(0, 0, 0, 0)');
  await navigation.getByRole('button',{name:'Commands',exact:true}).click();await expect(page.getByRole('heading',{name:'Commands'})).toBeVisible();await expect(page.locator('.project-settings > h2')).toHaveCount(0);await expect(page.locator('[name="command-settings"]')).toBeVisible();await expect(navigation.getByRole('button',{name:'Commands',exact:true})).toHaveAttribute('aria-current','page');
  await navigation.getByRole('button',{name:'Permissions'}).click();await expect(page.getByRole('heading',{name:'Permissions'})).toBeVisible();await expect(page.locator('[name="permission-automation-action"]')).toBeVisible();await expect(page.locator('[name="command-settings"]')).toHaveCount(0);
  await navigation.getByRole('button',{name:'Column view'}).click();await expect(page.getByRole('heading',{name:'Column view'})).toBeVisible();await expect(page.getByText('Hide Verified column')).toBeVisible();
  await page.screenshot({path:'/private/tmp/hs2-wsmx7c-settings-sidebar-wide.png',fullPage:true});await page.setViewportSize({width:1024,height:600});await expect(page.getByRole('heading',{name:'Column view'})).toBeVisible();await expect(placeholderToolbar).toHaveCSS('border-bottom-color','rgba(0, 0, 0, 0)');await page.screenshot({path:'/private/tmp/hs2-wsmx7c-settings-sidebar-floor.png',fullPage:true});await page.getByLabel('List view').click();await expect(selected).toHaveAttribute('data-selected','true');
});

test('renders exactly once when the long poll announces a permission request',async({page})=>{
  await mockProject(page);let pending:Array<{id:number;connection:string;tool:string;action:string;always_allow_supported:boolean}>=[];
  await page.route('**/permissions',route=>route.fulfill({json:pending}));
  const polls:Array<import('@playwright/test').Route>=[];let cursor=0;
  await page.route('**/ws/poll*',route=>{const since=new URL(route.request().url()).searchParams.get('since');if(since===null)return route.fulfill({json:{cursor,events:[],overflow:false}});polls.push(route)});
  await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();
  await page.waitForTimeout(100);await resetRenderMetrics(page);pending=[{id:77,connection:'codex-session',tool:'Bash',action:'cargo test',always_allow_supported:true}];
  await expect.poll(()=>polls.length).toBeGreaterThan(0);cursor+=1;await polls.shift()!.fulfill({json:{cursor,events:[{store:'',kind:'permission_asked',id:'77',slug:'Bash'}],overflow:false}});
  await expect(page.locator('[data-component="permission-request-popup"]')).toBeVisible();
  const metrics=await renderMetrics(page);expect(metrics?.passes).toBe(1);expect(metrics?.mutations).toBeGreaterThan(0);
});

test('records externally resolved empty-action permissions in notification history',async({page})=>{
  await mockProject(page);let pending=[{id:81,connection:'claude-tool-search',tool:'ToolSearch',action:'',always_allow_supported:true}],cursor=0;const polls:Array<import('@playwright/test').Route>=[];
  await page.route('**/permissions',route=>route.fulfill({json:pending}));
  await page.route('**/ws/poll*',route=>{const since=new URL(route.request().url()).searchParams.get('since');if(since===null)return route.fulfill({json:{cursor,events:[],overflow:false}});polls.push(route)});
  await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();const popup=page.locator('[data-component="permission-request-popup"]');await expect(popup).toContainText('Wants permission to use ToolSearch');await expect(popup.locator('.permission-request-card__details')).toHaveCount(0);
  pending=[];await expect.poll(()=>polls.length).toBeGreaterThan(0);cursor+=1;await polls.shift()!.fulfill({json:{cursor,events:[{store:'',kind:'permission_resolved',id:'81',slug:'',message:'allow:once'}],overflow:false}});await expect(popup).toHaveCount(0);
  await page.getByRole('button',{name:/Notifications view/}).click();await page.getByRole('button',{name:/Last 24 Hours/}).click();const previous=page.locator('[data-component="notification-center"] .permission-request-card--list').filter({hasText:'ToolSearch'});await expect(previous).toContainText('allowed permission');await expect(previous.locator('.permission-request-card__details')).toHaveCount(0);await expect(previous.locator('.permission-request-card__footer')).toHaveCount(0);await expect(previous).toHaveCSS('padding-bottom','16px');expect(await previous.evaluate(node=>{const card=node.getBoundingClientRect(),summary=node.querySelector('.permission-request-card__summary')!.getBoundingClientRect(),border=Number.parseFloat(getComputedStyle(node).borderBottomWidth);return Math.round(card.bottom-summary.bottom-border)})).toBe(16);await page.screenshot({path:'/private/tmp/hs2-f058ae-responded-notification-wide.png',fullPage:true});await page.getByRole('button',{name:'Hide notification inspector'}).click();await page.setViewportSize({width:760,height:844});await expect(previous).toBeInViewport();await expect(previous).toHaveCSS('padding-bottom','16px');await page.screenshot({path:'/private/tmp/hs2-f058ae-responded-notification-narrow.png',fullPage:true});
});

test('keeps healthy tickets usable and offers safe reveal plus AI repair recovery',async({page})=>{
  await mockProject(page);
  const recoveryRequests:{reveal?:unknown;repair?:unknown}={};
  const diagnostic={
    store:'git-local',
    store_path:'/work/demo.hs2',
    path:'/work/demo.hs2/tickets/01/01M1DNB977BK0NG7YJ77RVZXTV.md',
    id:'01M1DNB977BK0NG7YJ77RVZXTV',
    slug:'HS2-QQRY00',
    error:'unsupported content follows the bounded Notes section',
  };
  let corruptTickets:typeof diagnostic[]=[],cursor=0;
  const polls:Array<import('@playwright/test').Route>=[];
  await page.route('**/__hotsheet/projects/*/corrupt-tickets/reveal',async route=>{recoveryRequests.reveal=route.request().postDataJSON();await route.fulfill({json:{revealed:true}})});
  await page.route('**/corrupt-tickets/repair',async route=>{recoveryRequests.repair=route.request().postDataJSON();await route.fulfill({status:201,json:{...full,id:'repair-01',native_id:'repair-01',qualified_id:'git-local:repair-01',slug:'HS2-REPAIR',title:'Repair corrupt ticket HS2-QQRY00',category:'bug',priority:'high',up_next:true}})});
  await page.route('**/corrupt-tickets',route=>{void route.fulfill({json:corruptTickets})});
  await page.route('**/ws/poll*',route=>{const since=new URL(route.request().url()).searchParams.get('since');if(since===null)return route.fulfill({json:{cursor,events:[],overflow:false}});polls.push(route)});
  await page.goto('/');
  await page.getByRole('button',{name:'Open project'}).click();
  await page.getByRole('button',{name:'Open project',exact:true}).last().click();

  const stale=page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-QQRY00"]');
  await expect(stale).toBeVisible();
  corruptTickets=[diagnostic];
  await expect.poll(()=>polls.length).toBeGreaterThan(0);cursor+=1;await polls.shift()!.fulfill({json:{cursor,events:[{store:'git-local',kind:'changed',id:diagnostic.id,slug:diagnostic.slug}],overflow:false}});
  const corrupt=page.locator('[data-component="corrupt-ticket-row"]');
  await expect(page.getByRole('button',{name:/Ticket errors/})).toContainText('1');
  await expect(corrupt).toHaveCount(0);
  await page.getByRole('button',{name:/Ticket errors/}).click();
  await expect(corrupt).toContainText('HS2-QQRY00');
  await expect(stale).toHaveCount(0);
  await expect(corrupt).toContainText('Ticket file could not be read');
  await expect(corrupt).toHaveAttribute('role','group');
  await expect(corrupt.locator('[data-lucide="file-warning"]')).toBeVisible();
  await corrupt.getByRole('button',{name:'Open recovery for HS2-QQRY00'}).click();
  const inspector=page.locator('[data-component="corrupt-ticket-inspector"]');
  await expect(inspector).toContainText('Ticket parsing error');
  await expect(inspector).toContainText('unsupported content follows the bounded Notes section');
  await expect(inspector).toContainText('01M1DNB977BK0NG7YJ77RVZXTV.md');
  await inspector.getByRole('button',{name:'Reveal in Finder'}).click();
  await expect(inspector).toContainText('Opened the file location.');
  expect(recoveryRequests.reveal).toEqual({path:'/work/demo.hs2/tickets/01/01M1DNB977BK0NG7YJ77RVZXTV.md'});
  await inspector.getByRole('button',{name:'Attempt AI repair'}).click();
  await expect(inspector).toContainText('Queued HS2-REPAIR for AI repair.');
  expect(recoveryRequests.repair).toEqual({path:'/work/demo.hs2/tickets/01/01M1DNB977BK0NG7YJ77RVZXTV.md'});
  await page.screenshot({path:'/private/tmp/hs2-j1f744-corrupt-recovery-wide.png',fullPage:true});
  await page.setViewportSize({width:1024,height:844});await expect(inspector).toBeVisible();await page.screenshot({path:'/private/tmp/hs2-j1f744-corrupt-recovery-narrow.png',fullPage:true});await page.setViewportSize({width:1280,height:720});

  corruptTickets=[];
  await expect.poll(()=>polls.length).toBeGreaterThan(0);cursor+=1;await polls.shift()!.fulfill({json:{cursor,events:[{store:'git-local',kind:'changed',id:diagnostic.id,slug:diagnostic.slug}],overflow:false}});
  await expect(page.getByRole('button',{name:/Ticket errors/})).toHaveCount(0);
  await expect(page.getByRole('heading',{name:'Queue'})).toBeVisible();
  await expect(corrupt).toHaveCount(0);

  await page.getByText('Use real project tickets').click();
  await expect(page.locator('[data-component="ticket-inspector"]')).toContainText('Use real project tickets');
});

test('identifies a ticket from newer HS2 as upgrade-required instead of corrupt',async({page})=>{
  await mockProject(page);
  await page.route('**/corrupt-tickets',route=>route.fulfill({json:[{
    store:'git-local',store_path:'/work/demo.hs2',path:'/work/demo.hs2/tickets/01/new.md',
    slug:'HS2-NEWER',error_code:'upgrade_required',
    error:'This ticket was created by a newer version of Hot Sheet 2 and cannot be opened by this version. Update Hot Sheet 2 to open it.',
  }]}));
  await page.goto('/');
  await page.getByRole('button',{name:'Open project'}).click();
  await page.getByRole('button',{name:'Open project',exact:true}).last().click();
  await page.getByRole('button',{name:/Ticket errors/}).click();
  const newer=page.locator('[data-component="corrupt-ticket-row"]');
  await expect(newer).toContainText('Hot Sheet 2 update required');
  await expect(newer.locator('[data-lucide="refresh-cw"]')).toBeVisible();
  await expect(newer).not.toContainText('Ticket file could not be read');
  await newer.getByRole('button',{name:'Open recovery for HS2-NEWER'}).click();const inspector=page.locator('[data-component="corrupt-ticket-inspector"]');await expect(inspector).toContainText('created by a newer version');await expect(inspector.getByRole('button',{name:'Reveal in Finder'})).toBeVisible();
  await expect(inspector.getByRole('button',{name:'Attempt AI repair'})).toHaveCount(0);
});

test('updates the open project after external ticket additions edits and deletion',async({page})=>{
  await mockProject(page);
  let liveRows=[row,backlogRow,archiveRow,notStartedRow,completedRow,verifiedRow,startedRow2,startedRow3],liveFull={...full},cursor=0;
  const polls:Array<import('@playwright/test').Route>=[];
  await page.route('**/tickets',route=>route.request().method()==='GET'?route.fulfill({json:liveRows}):route.fallback());
  await page.route('**/tickets/01',route=>route.request().method()==='GET'?route.fulfill({json:{store:'git-local',...liveFull}}):route.fallback());
  await page.route('**/ws/poll*',route=>{const since=new URL(route.request().url()).searchParams.get('since');if(since===null)return route.fulfill({json:{cursor,events:[],overflow:false}});polls.push(route)});
  const emit=async(kind:string,id:string,slug:string)=>{await expect.poll(()=>polls.length).toBeGreaterThan(0);cursor+=1;await polls.shift()!.fulfill({json:{cursor,events:[{store:'git-local',kind,id,slug}],overflow:false}})};

  await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();
  const original=page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-DEMO01"]');await original.click();await expect(page.locator('[data-component="ticket-inspector"]')).toContainText('Use real project tickets');

  const openSelect=page.locator('[data-component="ticket-inspector"] wa-select[name="inspector-priority"]');await openSelect.click();await expect(openSelect).toHaveJSProperty('open',true);
  liveRows=liveRows.map(item=>item.id==='01'?{...item,title:'Externally edited ticket'}:item);liveFull={...liveFull,title:'Externally edited ticket'};await emit('changed','01','HS2-DEMO01');
  await page.waitForTimeout(200);await expect(original).toContainText('Use real project tickets');await expect(page.locator('.app-loading')).toBeHidden();await expect(openSelect).toHaveJSProperty('open',true);await page.keyboard.press('Escape');
  await expect(original).toContainText('Externally edited ticket');await expect(page.locator('[data-component="ticket-inspector"]')).toContainText('Externally edited ticket');

  const external={...row,id:'10',native_id:'10',qualified_id:'git-local:10',slug:'HS2-EXTERNAL',title:'Externally added ticket',status:'not_started',up_next:false};liveRows=[external,...liveRows];await emit('changed','10','HS2-EXTERNAL');
  await expect(page.locator('[data-ticket-slug="HS2-EXTERNAL"]')).toContainText('Externally added ticket');
  await page.screenshot({path:'/private/tmp/hs2-9d4hcq-live-wide.png',fullPage:true});
  await page.setViewportSize({width:390,height:844});await page.screenshot({path:'/private/tmp/hs2-9d4hcq-live-narrow.png',fullPage:true});await page.setViewportSize({width:1280,height:720});

  liveRows=liveRows.filter(item=>item.id!=='01');await emit('deleted','01','');
  await expect(original).toHaveCount(0);await expect(page.getByRole('complementary',{name:'Ticket inspector'})).toContainText('Select a ticket to see and edit its details');
});

test('merges unrelated external ticket fields and offers an editable merge for the active field',async({page})=>{
  await page.setViewportSize({width:1440,height:900});
  await mockProject(page);
  let liveRows=[row,backlogRow,archiveRow,notStartedRow,completedRow,verifiedRow,startedRow2,startedRow3],liveFull={...full},cursor=0;
  const polls:Array<import('@playwright/test').Route>=[],patches:Record<string,unknown>[]=[];
  await page.route('**/tickets',route=>route.request().method()==='GET'?route.fulfill({json:liveRows}):route.fallback());
  await page.route('**/tickets/01',route=>{
    const request=route.request();
    if(request.method()==='GET')return route.fulfill({json:{store:'git-local',...liveFull}});
    if(request.method()!=='PATCH')return route.fallback();
    const patch=request.postDataJSON() as Record<string,unknown>;
    patches.push(patch);
    if(patch.expected_token!==liveFull.concurrency_token)return route.fulfill({status:409,json:{error:'ticket changed since it was read'}});
    liveFull={...liveFull,...patch,concurrency_token:`committed-${patches.length}`};
    liveRows=liveRows.map(item=>item.id==='01'?{...item,...patch,updated_at:'2026-09-02T03:00:00Z'}:item);
    return route.fulfill({json:{store:'git-local',...liveFull}});
  });
  await page.route('**/ws/poll*',route=>{const since=new URL(route.request().url()).searchParams.get('since');if(since===null)return route.fulfill({json:{cursor,events:[],overflow:false}});polls.push(route)});
  const emit=async()=>{await expect.poll(()=>polls.length).toBeGreaterThan(0);cursor+=1;await polls.shift()!.fulfill({json:{cursor,events:[{store:'git-local',kind:'changed',id:'01',slug:'HS2-DEMO01'}],overflow:false}})};

  await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();await page.locator('[data-ticket-slug="HS2-DEMO01"]').click();
  const inspector=page.locator('[data-component="ticket-inspector"]');await inspector.getByRole('button',{name:'Edit Ticket details'}).dblclick();let editor=inspector.getByRole('textbox',{name:'Ticket details'});

  liveFull={...liveFull,details:'Remote-only details',concurrency_token:'remote-details'};await emit();
  await expect(editor).toHaveValue('Remote-only details');await expect(inspector.locator('[data-component="ticket-field-conflict"]')).toHaveCount(0);

  liveFull={...liveFull,status:'completed',concurrency_token:'remote-status'};liveRows=liveRows.map(item=>item.id==='01'?{...item,status:'completed'}:item);await editor.fill('Local text after remote status');
  await expect.poll(()=>patches.some(patch=>patch.details==='Local text after remote status'&&patch.expected_token==='remote-status')).toBe(true);
  await expect(inspector.locator('wa-select[name="inspector-status"]')).toHaveJSProperty('value','completed');await expect(editor).toHaveValue('Local text after remote status');await expect(inspector.locator('[data-component="ticket-field-conflict"]')).toHaveCount(0);await emit();

  await inspector.getByRole('button',{name:'Open ticket reader'}).evaluate(node=>{(node as HTMLElement).click()});const reader=page.getByRole('dialog',{name:/Read and edit HS2-DEMO01/});await expect(reader).toBeVisible();await reader.getByRole('button',{name:'Edit Ticket details'}).dblclick();editor=reader.getByRole('textbox',{name:'Ticket details'});await editor.fill('My local wording');await expect.poll(()=>patches.some(patch=>patch.details==='My local wording')).toBe(true);
  liveFull={...liveFull,details:'Their newer wording',concurrency_token:'remote-conflict'};await editor.fill('My revised local wording');
  const conflict=reader.locator('[data-component="ticket-field-conflict"]');await expect(conflict).toBeVisible();await expect(conflict).toContainText('Their newer wording');await expect(conflict).toContainText('My revised local wording');await expect(page.locator('.app-error')).toHaveCount(0);
  await page.screenshot({path:'/private/tmp/hs2-0bp930-field-conflict-wide.png',fullPage:true});await page.setViewportSize({width:760,height:940});await expect(conflict).toBeVisible();await page.screenshot({path:'/private/tmp/hs2-0bp930-field-conflict-narrow.png',fullPage:true});

  const resolution=conflict.getByRole('textbox',{name:'Merged details'});await resolution.fill('My local wording\n\nTheir newer wording');await conflict.getByRole('button',{name:'Apply merged value'}).click();
  await expect(conflict).toHaveCount(0);await expect(editor).toHaveValue('My local wording\n\nTheir newer wording');
  await expect.poll(()=>patches.some(patch=>patch.details==='My local wording\n\nTheir newer wording'&&patch.expected_token==='remote-conflict')).toBe(true);
});

test('does not report this clients own in-flight autosave as a merge conflict',async({page})=>{
  await mockProject(page);let liveFull={...full},liveRows=[row],cursor=0;const polls:Array<import('@playwright/test').Route>=[],writes:Array<import('@playwright/test').Route>=[];
  await page.route('**/tickets',route=>route.request().method()==='GET'?route.fulfill({json:liveRows}):route.fallback());
  await page.route('**/tickets/01',route=>{if(route.request().method()==='GET')return route.fulfill({json:{store:'git-local',...liveFull}});if(route.request().method()==='PATCH'){writes.push(route);return}return route.fallback()});
  await page.route('**/ws/poll*',route=>{const since=new URL(route.request().url()).searchParams.get('since');if(since===null)return route.fulfill({json:{cursor,events:[],overflow:false}});polls.push(route)});
  const emit=async()=>{await expect.poll(()=>polls.length).toBeGreaterThan(0);cursor+=1;await polls.shift()!.fulfill({json:{cursor,events:[{store:'git-local',kind:'changed',id:'01',slug:'HS2-DEMO01'}],overflow:false}})};
  await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();await page.locator('[data-ticket-slug="HS2-DEMO01"]').click();const inspector=page.locator('[data-component="ticket-inspector"]');await inspector.getByRole('button',{name:'Edit Ticket details'}).dblclick();const editor=inspector.getByRole('textbox',{name:'Ticket details'});
  const partial='they should have round borders and outl',complete='they should have round borders and outlines';await editor.fill(partial);await expect.poll(()=>writes.length).toBe(1);await editor.fill(complete);await page.waitForTimeout(300);expect(writes).toHaveLength(1);
  const first=writes.shift()!;liveFull={...liveFull,details:partial,concurrency_token:'partial-token'};liveRows=liveRows.map(item=>({...item,details:partial}));await first.fulfill({json:{store:'git-local',...liveFull}});await expect.poll(()=>writes.length).toBe(1);const second=writes.shift()!;expect(second.request().postDataJSON().expected_token).toBe('partial-token');await emit();await expect(editor).toHaveValue(complete);await expect(inspector.locator('[data-component="ticket-field-conflict"]')).toHaveCount(0);
  liveFull={...liveFull,details:complete,concurrency_token:'complete-token'};await second.fulfill({json:{store:'git-local',...liveFull}});await expect(editor).toHaveValue(complete);await expect(inspector.locator('[data-component="ticket-field-conflict"]')).toHaveCount(0);
});

test('translates urgent priority through the canonical server contract',async({page})=>{
  const patches=await mockProject(page);await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();await page.getByText('Use real project tickets').click();
  await page.locator('wa-select[name="inspector-priority"]').evaluate((node:HTMLElement&{value:string})=>{node.value='urgent';node.dispatchEvent(new Event('change',{bubbles:true}))});
  await expect.poll(()=>patches.at(-1)?.priority).toBe('highest');
  await expect(page.locator('[data-component="ticket-inspector"] wa-select[name="inspector-priority"]')).toHaveJSProperty('value','urgent');
  await expect(page.locator('[data-component="ticket-inspector"] wa-select[name="inspector-priority"] .select__icon--selected [data-lucide="chevrons-up"]')).toBeVisible();
});

test('moves tickets to Backlog and Archive from every shipped status menu',async({page})=>{
  const patches=await mockProject(page);await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();
  await page.getByRole('button',{name:/Archive/}).click();const archived=page.locator('[data-ticket-slug="HS2-ARCH01"]');await archived.click();
  const inspector=page.locator('[data-component="ticket-inspector"]'),status=inspector.locator('wa-select[name="inspector-status"]');
  await expect(status).toHaveJSProperty('value','archive');await expect(status).toHaveAttribute('aria-label','Change status, Archive');await expect(status.locator('.select__custom-selected [data-lucide="archive"]')).toBeVisible();
  await status.click();await expect(status.locator('wa-option')).toHaveCount(6);await expect(status.locator('wa-divider')).toHaveCount(1);await status.locator('wa-option[value="backlog"]').click();
  await expect.poll(()=>patches.some(patch=>patch.status==='backlog')).toBe(true);await page.getByRole('button',{name:/Backlog/}).click();const backlogged=page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-ARCH01"]');await expect(backlogged).toBeVisible();
  await backlogged.click({button:'right'});const menu=page.getByRole('menu',{name:'Ticket actions'});await menu.locator('wa-dropdown-item:not([slot="submenu"])',{hasText:'Change status'}).hover();
  const choices=menu.locator('[data-context-field="status"]');await expect(choices).toHaveCount(6);await expect(menu.locator('wa-divider[slot="submenu"]')).toHaveCount(1);await choices.filter({hasText:'Archive'}).click();
  await expect.poll(()=>patches.some(patch=>patch.status==='archive')).toBe(true);await page.getByRole('button',{name:/Archive/}).click();await expect(page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-ARCH01"]')).toBeVisible();
});

test('projects Up Next immediately and reconciles without a full project refresh',async({page})=>{
  await mockProject(page);await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();
  const row=page.locator('[data-component="ticket-list-row"]',{hasText:'Not started ticket'});const requests:string[]=[];page.on('request',request=>{requests.push(`${request.method()} ${new URL(request.url()).pathname}`)});
  await page.route('**/tickets/05',async route=>{if(route.request().method()!=='PATCH')return route.fallback();await new Promise(resolve=>setTimeout(resolve,250));const body=route.request().postDataJSON();return route.fulfill({json:{store:'git-local',...notStartedRow,...body,details:'',notes:[],attachments:[]}})});
  await page.evaluate(()=>{document.addEventListener('click',()=>{const started=performance.now();requestAnimationFrame(()=>{(window as typeof window&{mutationRenderMs?:number}).mutationRenderMs=performance.now()-started})},{once:true,capture:true})});
  await row.getByRole('button',{name:'Add to Up Next'}).click();
  await expect(row.getByRole('button',{name:'Remove from Up Next'})).toBeVisible();await expect.poll(()=>page.evaluate(()=>(window as typeof window&{mutationRenderMs?:number}).mutationRenderMs)).toBeLessThan(100);
  await expect.poll(()=>requests.some(value=>value.startsWith('PATCH ')&&!value.endsWith('/tickets'))).toBe(true);await page.waitForTimeout(300);
  const afterPatch=requests.slice(requests.findIndex(value=>value.startsWith('PATCH '))+1);expect(afterPatch.filter(value=>value.includes('/tickets')||value.includes('/repository/status'))).toEqual([]);
});

test('autosaves ticket text fields without explicit save or cancel controls',async({page})=>{
  const patches=await mockProject(page);await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();await page.getByText('Use real project tickets').click();
  const inspector=page.locator('[data-component="ticket-inspector"]');
  await inspector.getByRole('button',{name:'Edit Ticket details'}).dblclick();
  const details=inspector.getByRole('textbox',{name:'Ticket details'});await details.fill('Autosaved details');
  await expect.poll(()=>patches.some(patch=>patch.details==='Autosaved details')).toBe(true);
  await expect(inspector.getByRole('button',{name:/Save|Cancel/})).toHaveCount(0);

  const note=inspector.locator('[data-component="note-card"][data-note-id="N3"]');await note.dblclick();const noteEditor=note.getByRole('textbox',{name:'Note body'});await noteEditor.fill('Autosaved note');
  await expect.poll(()=>patches.some(patch=>patch.note_id==='N3'&&patch.note==='Autosaved note')).toBe(true);

  await inspector.locator('.ticket-inspector__content').evaluate(node=>{node.scrollTop=0});await inspector.screenshot({path:'/private/tmp/hs2-qbscn2-block-ticket-empty.png'});await inspector.getByRole('button',{name:'Block ticket'}).click();const blocked=inspector.getByRole('textbox',{name:'Blocked reason'});await blocked.fill('Waiting for review');
  await expect.poll(()=>patches.some(patch=>patch.blocked_reason==='Waiting for review')).toBe(true);await blocked.blur();await expect(inspector.getByText('Waiting for review',{exact:true})).toBeVisible();await expect(inspector.getByRole('heading',{name:'Blocked reason'})).toBeVisible();await inspector.screenshot({path:'/private/tmp/hs2-72kryh-blocked-reason.png'});
  await inspector.locator('.ticket-inspector__blocked-surface').dblclick();const clearReason=inspector.getByRole('textbox',{name:'Blocked reason'});await expect(clearReason).toBeFocused();await clearReason.fill('   ');await clearReason.blur();await expect.poll(()=>patches.some(patch=>patch.blocked_reason===null)).toBe(true);await expect(inspector.getByRole('button',{name:'Block ticket'})).toBeVisible();
});

test('creates, cancels, edits, and deletes notes through the shared inspector and reader',async({page})=>{
  const patches=await mockProject(page);await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();await page.getByText('Use real project tickets').click();
  const inspector=page.locator('[data-component="ticket-inspector"]');await inspector.getByRole('button',{name:'Add note'}).first().click();const composer=inspector.locator('[data-component="note-composer"]');await expect(composer.getByRole('textbox',{name:'New note'})).toBeFocused();await composer.getByRole('textbox',{name:'New note'}).fill('Discard me');await composer.getByRole('button',{name:'Cancel'}).click();await expect(composer).toHaveCount(0);
  await inspector.getByRole('button',{name:'Add note'}).first().click();await inspector.getByRole('textbox',{name:'New note'}).fill('Created note');await inspector.getByRole('button',{name:'Add note',exact:true}).last().click();await expect.poll(()=>patches.some(patch=>patch.note==='Created note'&&patch.note_kind==='regular')).toBe(true);
  const feedbackSurface=inspector.locator('[data-note-id="N2"] .note-card__body');await feedbackSurface.dblclick();const feedbackEditor=inspector.getByRole('textbox',{name:'Note body'});await expect(feedbackEditor).toHaveValue('Should this reader preserve the current draft?');await feedbackEditor.fill('Revised feedback question');await expect.poll(()=>patches.some(patch=>patch.note_id==='N2'&&patch.note==='Revised feedback question')).toBe(true);await feedbackEditor.blur();
  const noteSurface=inspector.locator('[data-note-id="N3"] .note-card__body');await expect(noteSurface).toHaveAttribute('aria-label','Edit note');await noteSurface.dblclick();const editor=inspector.getByRole('textbox',{name:'Note body'});await editor.fill('Edited lifecycle note');await expect.poll(()=>patches.some(patch=>patch.note_id==='N3'&&patch.note==='Edited lifecycle note')).toBe(true);await editor.blur();await expect(editor).toHaveCount(0);
  await inspector.locator('[data-note-id="N3"]').getByRole('button',{name:'Delete note'}).click();await expect(inspector.locator('[data-note-id="N3"]')).toHaveCount(0);
  await inspector.getByRole('button',{name:'Open ticket reader'}).click();const reader=page.getByRole('dialog');await expect(reader.getByRole('button',{name:'Edit ticket',exact:true})).toHaveCount(0);const readerDetails=reader.getByRole('button',{name:'Edit Ticket details'});await expect(readerDetails).toBeVisible();await readerDetails.dblclick();const readerDetailsEditor=reader.getByRole('textbox',{name:'Ticket details'});await expect(readerDetailsEditor).toBeFocused();await readerDetailsEditor.blur();await reader.getByRole('button',{name:'Add note'}).first().click();const readerComposer=reader.locator('[data-component="note-composer"]');await expect(readerComposer).toBeVisible();await expect(readerComposer.getByRole('textbox',{name:'New note'})).toBeFocused();const firstCard=await reader.locator('[data-component="note-card"]').first().elementHandle();expect(await readerComposer.evaluate((composer,list)=>Boolean(list.compareDocumentPosition(composer)&Node.DOCUMENT_POSITION_FOLLOWING),firstCard)).toBe(true);
});

test('offers a visible Add note action before the first note exists',async({page})=>{
  await page.setViewportSize({width:1280,height:720});await mockProject(page);await page.route('**/tickets/01',route=>route.request().method()==='GET'?route.fulfill({json:{store:'git-local',...full,notes:[]}}):route.fallback());await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();await page.getByText('Use real project tickets').click();
  const inspector=page.locator('[data-component="ticket-inspector"]');const add=inspector.locator('.ticket-notes__add');await expect(add).toBeVisible();await expect(add).toHaveText(/Add note/);await page.screenshot({path:'/private/tmp/hs2-yn3x2j-empty-notes-wide.png',fullPage:true});await page.setViewportSize({width:940,height:844});await expect(add).toBeVisible();await page.screenshot({path:'/private/tmp/hs2-yn3x2j-empty-notes-narrow.png',fullPage:true});await add.click();await expect(inspector.getByRole('textbox',{name:'New note'})).toBeFocused();
});

test('edits title and tags through controlled capability-aware inspector state',async({page})=>{
  const patches=await mockProject(page);await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();await page.getByText('Use real project tickets').click();
  const inspector=page.locator('[data-component="ticket-inspector"]');const title=inspector.getByRole('heading',{name:'Use real project tickets'});await title.dblclick();const titleInput=inspector.getByRole('textbox',{name:'Ticket title'});await titleInput.fill('Renamed ticket');await expect.poll(()=>patches.some(patch=>patch.title==='Renamed ticket')).toBe(true);await titleInput.blur();await expect(inspector.getByRole('heading',{name:'Renamed ticket'})).toBeVisible();
  const tagInput=inspector.getByRole('combobox',{name:'Add tag'});await tagInput.fill('regression');await tagInput.press('Enter');await expect(inspector.locator('[data-component="tag-chip"][data-tag-id="regression"]')).toBeVisible();await expect.poll(()=>patches.some(patch=>Array.isArray(patch.tags)&&patch.tags.includes('regression'))).toBe(true);
  await inspector.locator('[data-component="tag-chip"][data-tag-id="client"]').evaluate(node=>node.dispatchEvent(new CustomEvent('wa-remove',{bubbles:true})));await expect(inspector.locator('[data-component="tag-chip"][data-tag-id="client"]')).toHaveCount(0);await expect.poll(()=>patches.some(patch=>Array.isArray(patch.tags)&&!patch.tags.includes('client'))).toBe(true);
});

test('hides title and tag mutation affordances when the provider cannot update',async({page})=>{
  await mockProject(page,false);await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();await page.getByText('Use real project tickets').click();const inspector=page.locator('[data-component="ticket-inspector"]');
  await inspector.getByRole('heading',{name:'Use real project tickets'}).dblclick();await expect(inspector.getByRole('textbox',{name:'Ticket title'})).toHaveCount(0);await expect(inspector.getByRole('combobox',{name:'Add tag'})).toHaveCount(0);await expect(inspector.locator('[data-component="tag-chip"]')).not.toHaveAttribute('with-remove','');await expect(inspector.getByRole('button',{name:'Edit note'})).toHaveCount(0);await expect(inspector.getByRole('button',{name:'Delete note'})).toHaveCount(0);
});

test('opens a checkout, discovers its source, and drives real shell ticket flows',async({page})=>{
  await mockProject(page);let submittedTitle='';page.on('request',request=>{if(request.method()==='POST'&&new URL(request.url()).pathname.endsWith('/tickets'))submittedTitle=request.postDataJSON().title});await page.goto('/');
  await page.getByRole('button',{name:'Open project'}).click();
  await expect(page.locator('wa-input[name="project-root"]')).toHaveJSProperty('value','/Users/westphal/Documents/hotsheet2');
  await page.getByRole('button',{name:'Open project',exact:true}).last().click();
  await expect(page.getByText('Use real project tickets')).toBeVisible();
  const featureRow=page.locator('[data-component="ticket-list-row"]',{hasText:'Use real project tickets'});
  await expect(featureRow.locator('[data-lucide="sparkles"]')).toBeVisible();
  await expect(featureRow.locator('.ticket-list-row__category--label')).toHaveCount(0);
  await page.getByText('Use real project tickets').click();
  await expect(page.getByText('The real ticket body.')).toBeVisible();
  await page.locator('wa-select[name="inspector-status"]').click();
  await page.locator('wa-select[name="inspector-status"] wa-option[value="completed"]').click();
  await expect(page.locator('[data-component="ticket-inspector"] [data-component="status-badge"]')).toContainText('Completed');
  await page.getByRole('button',{name:'Timeline'}).click();
  await expect(page.getByText('Ticket created')).toBeVisible();
  await expect(page.getByText('Connected the client')).toBeVisible();
  const timeline=page.locator('[data-component="ticket-timeline"]');await expect(timeline.getByText('Completed',{exact:true})).toBeVisible();await expect(timeline).not.toContainText('Status changed from Started to Completed');
  await page.getByRole('button',{name:'Info'}).click();await page.locator('wa-select[name="inspector-status"]').click();await page.locator('wa-select[name="inspector-status"] wa-option[value="backlog"]').click();await page.getByRole('button',{name:'Timeline'}).click();await expect(timeline.getByText('Moved to backlog',{exact:true})).toBeVisible();await expect(timeline).not.toContainText('Status changed from Completed to Backlog');
  await page.screenshot({path:'/private/tmp/hs2-22gcky-timeline-wide.png'});
  await page.setViewportSize({width:940,height:900});
  await expect(page.locator('[data-component="ticket-timeline"]')).toBeVisible();
  await page.screenshot({path:'/private/tmp/hs2-22gcky-timeline-narrow.png'});
  await page.getByRole('button',{name:'Info'}).click();
  await page.getByRole('button',{name:'New ticket…'}).click();
  await page.locator('wa-input[name="new-ticket-title"]').evaluate((node:HTMLElement&{value:string})=>{node.value='[client] [Needs Review] Created from the real shell';node.dispatchEvent(new Event('input',{bubbles:true}))});
  await page.getByRole('button',{name:'Create ticket'}).click();
  const createdRow=page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-NEW001"]');
  await expect(createdRow).toContainText('Created from the real shell');
  await expect(createdRow).not.toContainText('[client]');
  expect(submittedTitle).toBe('[client] [Needs Review] Created from the real shell');
  await expect(createdRow).toHaveAttribute('data-selected','true');
  await expect(page.locator('[data-component="ticket-inspector"]')).toContainText('HS2-NEW001');
  await expect(page.locator('[data-component="ticket-inspector"] [data-component="markdown-editor"]')).toHaveAttribute('data-mode','write');
  await expect(page.getByRole('textbox',{name:'Details'})).toBeFocused();
  await expect(page.locator('[data-component="ticket-list-row"][data-selected="true"]')).toHaveCount(1);
  await expect(page.locator('[data-component="ticket-inspector"]')).toContainText('client');
  await expect(page.locator('[data-component="ticket-inspector"]')).toContainText('Needs-Review');
  await page.screenshot({path:'/private/tmp/hs2-chzkr5-create-wide.png',fullPage:true});
  await page.setViewportSize({width:760,height:900});await page.screenshot({path:'/private/tmp/hs2-chzkr5-create-narrow.png',fullPage:true});
  await page.getByLabel('Settings view').click();
  await expect(page.getByText('/work/demo.hs2')).toBeVisible();
});

test('holds the production AppShell at its 1024 by 600 supported floor',async({page})=>{
  await page.setViewportSize({width:800,height:500});await mockProject(page);await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();await expect(page.locator('[data-project-dialog]')).toBeHidden();
  const shell=page.locator('[data-component="app-shell"]'),bounds=await shell.boundingBox();expect(bounds?.width).toBeGreaterThanOrEqual(1024);expect(bounds?.height).toBeGreaterThanOrEqual(600);await expect(shell.locator('[data-component="resizable-region"][data-region-id="app-sidebar"]')).toBeVisible();await expect(shell.locator('[data-component="resizable-region"][data-region-id="app-inspector"]')).toBeVisible();await page.setViewportSize({width:1024,height:600});await page.waitForTimeout(300);await page.screenshot({path:'/private/tmp/hs2-501eph-production-shell-floor.png',fullPage:true});
});

test('shows reactive open and Up Next counts immediately above Drive',async({page})=>{
  await mockProject(page);await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();await expect(page.locator('[data-project-dialog]')).toBeHidden();await page.waitForTimeout(400);const sidebar=page.locator('[data-component="project-sidebar"]'),summary=sidebar.locator('[data-component="project-work-summary"]'),drive=sidebar.locator('[data-component="drive-control"]');await expect(summary).toHaveText('6 open, 1 up next, 0 active');const geometry=await sidebar.evaluate(node=>{const sidebarBox=node.getBoundingClientRect(),summaryBox=node.querySelector('[data-component="project-work-summary"]')!.getBoundingClientRect(),driveBox=node.querySelector('[data-component="drive-control"]')!.getBoundingClientRect();return{centerDelta:Math.abs((summaryBox.left+summaryBox.width/2)-(sidebarBox.left+sidebarBox.width/2)),orderGap:driveBox.top-summaryBox.bottom}});expect(geometry.centerDelta).toBeLessThan(1);expect(geometry.orderGap).toBeGreaterThanOrEqual(0);await page.screenshot({path:'/private/tmp/hs2-a94d3h-project-work-summary-wide.png',fullPage:true});
  await page.locator('[data-ticket-slug="HS2-NEXT01"]').click();const statusSelect=page.locator('wa-select[name="inspector-status"]');await statusSelect.click();await statusSelect.locator('wa-option[value="completed"]').click();await expect(summary).toHaveText('5 open, 1 up next, 0 active');await page.keyboard.press('Escape');await page.setViewportSize({width:940,height:844});await expect(summary).toBeVisible();await expect(drive).toBeVisible();expect(await summary.evaluate(node=>node.scrollWidth<=node.clientWidth)).toBe(true);await page.screenshot({path:'/private/tmp/hs2-a94d3h-project-work-summary-narrow.png',fullPage:true});
});

test('focuses the ticket title every time the real composer expands',async({page})=>{
  await mockProject(page);await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();const launcher=page.getByRole('button',{name:'New ticket…'});await launcher.click();const title=page.locator('wa-input[name="new-ticket-title"]');await expect(title).toBeFocused();await page.getByRole('button',{name:'Cancel'}).click();await launcher.click();await expect(title).toBeFocused();
});

test('stages safe attachment drops from the collapsed and expanded new-ticket composer',async({page})=>{
  await mockProject(page);const uploads:string[]=[];page.on('request',request=>{if(request.method()==='POST'&&new URL(request.url()).pathname.match(/\/tickets\/02\/attachments$/))uploads.push(decodeURIComponent(request.headers()['x-hotsheet-filename']??''))});await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();const launcher=page.getByRole('button',{name:'New ticket…'});
  await launcher.evaluate(node=>{const transfer=new DataTransfer();transfer.items.add(new File(['first proof'],'first-proof.txt',{type:'text/plain'}));node.dispatchEvent(new DragEvent('dragover',{bubbles:true,cancelable:true,dataTransfer:transfer}));});await expect(launcher).toHaveAttribute('data-dragging','true');await launcher.evaluate(node=>{const transfer=new DataTransfer();transfer.items.add(new File(['first proof'],'first-proof.txt',{type:'text/plain'}));node.dispatchEvent(new DragEvent('drop',{bubbles:true,cancelable:true,dataTransfer:transfer}));});const form=page.locator('[data-action="create-ticket-form"]');await expect(form.getByText('first-proof.txt')).toBeVisible();await form.screenshot({path:'/private/tmp/hs2-v1xn4t-new-ticket-drop-wide.png'});
  await form.evaluate(node=>{const transfer=new DataTransfer();transfer.items.add(new File(['second proof'],'second-proof.txt',{type:'text/plain'}));node.dispatchEvent(new DragEvent('drop',{bubbles:true,cancelable:true,dataTransfer:transfer}));});await expect(form.getByText('second-proof.txt')).toBeVisible();await form.getByRole('button',{name:'Remove first-proof.txt'}).click();await expect(form.getByText('first-proof.txt')).toHaveCount(0);await page.setViewportSize({width:940,height:844});await form.screenshot({path:'/private/tmp/hs2-v1xn4t-new-ticket-drop-narrow.png'});
  await form.getByRole('button',{name:'Cancel'}).click();await launcher.click();await expect(form.getByText('second-proof.txt')).toHaveCount(0);await form.getByLabel('Browse attachments for new ticket',{exact:true}).setInputFiles({name:'final-proof.txt',mimeType:'text/plain',buffer:Buffer.from('final proof')});await expect(form.getByText('final-proof.txt')).toBeVisible();await form.locator('wa-input[name="new-ticket-title"]').evaluate((node:HTMLElement&{value:string})=>{node.value='Created with dropped evidence';node.dispatchEvent(new Event('input',{bubbles:true}))});await form.getByRole('button',{name:'Create ticket'}).click();await expect.poll(()=>uploads).toEqual(['final-proof.txt']);await expect(page.locator('[data-component="ticket-inspector"]')).toContainText('HS2-NEW001');await page.getByRole('button',{name:'Attachments, 1'}).click();await expect(page.locator('[data-component="ticket-inspector"]')).toContainText('final-proof.txt');
});

test('renders attachment identity from a selected real ticket',async({page})=>{
  await mockProject(page);await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();await expect(page.locator('[data-project-dialog]')).toBeHidden();await page.locator('[data-ticket-slug="HS2-DEMO01"]').click();await expect(page.locator('[data-component="ticket-inspector"]')).toContainText('Use real project tickets');await page.locator('[data-component="ticket-inspector"]').evaluate((node:HTMLElement)=>{node.style.width='250px'});await expect(page.getByRole('button',{name:'Attachments, 1'}).locator('.ticket-inspector__tab-count')).toHaveText('1');await page.getByRole('button',{name:'Attachments, 1'}).click();
  const item=page.locator('[data-attachment-id="A1"]');await expect(item).toContainText('proof.png');
  const open=item.getByRole('button',{name:'Open proof.png'});for(const [name,title] of [['Open proof.png','Open proof.png'],['Download proof.png','Download proof.png'],['Copy reference to proof.png','Copy reference to proof.png'],['Remove proof.png','Remove proof.png']] as const)await expect(item.getByRole('button',{name})).toHaveAttribute('title',title);
  await page.evaluate(()=>{const opened:string[]=[];(window as typeof window&{__attachmentOpenUrls:string[]}).__attachmentOpenUrls=opened;window.open=(url?:string|URL)=>{opened.push(String(url));return null}});
  await open.click();const clickUrls=await page.evaluate(()=>(window as typeof window&{__attachmentOpenUrls:string[]}).__attachmentOpenUrls);expect(clickUrls).toHaveLength(1);await page.evaluate(()=>{(window as typeof window&{__attachmentOpenUrls:string[]}).__attachmentOpenUrls.length=0});await item.dblclick({position:{x:40,y:20}});const doubleClickUrls=await page.evaluate(()=>(window as typeof window&{__attachmentOpenUrls:string[]}).__attachmentOpenUrls);expect(doubleClickUrls).toEqual(clickUrls);
  await open.hover();await expect(open).not.toHaveCSS('background-color','rgba(0, 0, 0, 0)');await page.locator('[data-component="ticket-inspector"]').screenshot({path:'/private/tmp/hs2-pngaw7-gwtr5e-attachment-actions-wide.png'});await page.setViewportSize({width:940,height:844});await item.scrollIntoViewIfNeeded();await item.getByRole('button',{name:'Copy reference to proof.png'}).hover();await page.locator('[data-component="ticket-inspector"]').screenshot({path:'/private/tmp/hs2-pngaw7-gwtr5e-attachment-actions-narrow.png'});await page.setViewportSize({width:1280,height:720});
  const filename=item.locator(':scope > span').first();await filename.evaluate(node=>{node.textContent='an-extremely-long-attachment-filename-that-must-ellipsis-before-the-actions.png'});const inspector=await page.locator('[data-component="ticket-inspector"]').elementHandle();await expect.poll(async()=>item.evaluate((node,container)=>node.getBoundingClientRect().right<=container.getBoundingClientRect().right,inspector)).toBe(true);await expect.poll(()=>filename.evaluate(node=>node.scrollWidth>node.clientWidth)).toBe(true);await expect(item.getByRole('button')).toHaveCount(4);
  await page.getByLabel('Browse and add attachments').setInputFiles({name:'new-proof.txt',mimeType:'text/plain',buffer:Buffer.from('proof')});
  await expect(page.locator('[data-attachment-id="A2"]')).toContainText('new-proof.txt');
  await page.locator('[data-component="ticket-inspector"]').evaluate(node=>{const transfer=new DataTransfer();transfer.items.add(new File(['drop proof'],'dropped-proof.txt',{type:'text/plain'}));node.dispatchEvent(new DragEvent('drop',{bubbles:true,dataTransfer:transfer}))});
  await expect(page.locator('[data-attachment-id="A3"]')).toContainText('dropped-proof.txt');
  await page.locator('[data-component="ticket-inspector"]').evaluate(node=>{class PromisedFile extends File { override arrayBuffer(){return Promise.reject(new TypeError('backing file is unavailable'))} }const transfer=new DataTransfer();transfer.items.add(new PromisedFile(['pending'],'floating-capture.png',{type:'image/png'}));node.dispatchEvent(new DragEvent('drop',{bubbles:true,dataTransfer:transfer}))});
  await expect(page.getByRole('alert')).toContainText('floating-capture.png');
  await expect(page.getByRole('alert')).toContainText('Wait for it to appear on the desktop');
  await expect(page.locator('[data-attachment-id="A4"]')).toHaveCount(0);
  await page.getByRole('button',{name:'Remove new-proof.txt'}).click();
  await expect(page.locator('[data-attachment-id="A2"]')).toHaveCount(0);
  await expect(page.getByText('Attachment removed.')).toBeVisible();
});

test('edits non-empty details on double click and empty details on one click',async({page})=>{
  await mockProject(page);await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();await page.getByText('Use real project tickets').click();
  const preview=page.getByRole('button',{name:'Edit Ticket details'});
  await preview.dblclick();
  const source=page.getByRole('textbox',{name:'Ticket details'});await expect(source).toBeFocused();await source.fill('Carried into the larger editor');const detailsSurface=page.locator('.ticket-inspector__details-surface');const editorGeometry=await detailsSurface.evaluate((surface)=>{const editor=surface.querySelector<HTMLTextAreaElement>('textarea[name="markdown-source"]')!,outer=surface.getBoundingClientRect(),inner=editor.getBoundingClientRect(),style=getComputedStyle(editor);return{left:inner.left-outer.left,top:inner.top-outer.top,right:outer.right-inner.right,bottom:outer.bottom-inner.bottom,padding:style.padding,resize:style.resize}});expect(editorGeometry).toEqual({left:1,top:1,right:1,bottom:1,padding:'12px',resize:'vertical'});await detailsSurface.screenshot({path:'/private/tmp/hs2-7nzkyc-details-editor-wide.png'});await page.getByRole('button',{name:'Open ticket reader'}).click();
  const reader=page.getByRole('dialog',{name:/Read and edit HS2-DEMO01/});await expect(reader).toBeVisible();const readerPreview=reader.getByRole('button',{name:'Edit Ticket details'});await expect(readerPreview).toContainText('Carried into the larger editor');await readerPreview.dblclick();await expect(reader.getByRole('textbox',{name:'Ticket details'})).toHaveValue('Carried into the larger editor');await expect(reader.getByRole('textbox',{name:'Feedback response'})).toHaveCount(0);await expect(reader.locator('article[data-note-id="N2"]')).toHaveAttribute('data-kind','regular');await reader.getByRole('button',{name:'Close ticket reader'}).click();
  await expect(source).toHaveCount(0);await page.getByRole('button',{name:'Edit Ticket details'}).dblclick();await source.fill('');await source.blur();
  await expect(page.getByText('Click to add Markdown.')).toBeVisible();
  await page.getByRole('button',{name:'Edit Ticket details'}).click();
  await expect(source).toBeFocused();await source.fill('Added from an empty ticket');await source.blur();
  await expect(page.locator('.ticket-inspector__details-surface [data-component="markdown-preview"]')).toContainText('Added from an empty ticket');await page.getByRole('button',{name:'Edit Ticket details'}).dblclick();await page.setViewportSize({width:1024,height:600});await detailsSurface.screenshot({path:'/private/tmp/hs2-7nzkyc-details-editor-narrow.png'});
});

test('keeps reader details, blocked reason, and note edit state independent from the sidebar',async({page})=>{
  const patches=await mockProject(page,true,false,0,0,300);await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();const row=page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-DEMO01"]');await row.dblclick();let reader=page.getByRole('dialog',{name:'Read and edit HS2-DEMO01'}),preview=reader.getByRole('button',{name:'Edit Ticket details'});const sidebar=page.locator('[data-region-id="app-inspector"]');await preview.dblclick();let source=reader.getByRole('textbox',{name:'Ticket details'});await expect(source).toBeFocused();await expect(sidebar.getByRole('textbox',{name:'Ticket details'})).toHaveCount(0);await source.blur();await expect(reader.getByRole('textbox',{name:'Ticket details'})).toHaveCount(0);await expect(preview).toContainText('The real ticket body.');
  await reader.getByRole('button',{name:'Block ticket'}).click();await expect(reader.getByRole('textbox',{name:'Blocked reason'})).toBeVisible();await expect(sidebar.getByRole('textbox',{name:'Blocked reason'})).toHaveCount(0);await reader.getByRole('textbox',{name:'Blocked reason'}).blur();
  await reader.getByText('Editable note with').dblclick();await expect(reader.getByRole('textbox',{name:'Note body'})).toBeVisible();await expect(sidebar.getByRole('textbox',{name:'Note body'})).toHaveCount(0);await reader.getByRole('textbox',{name:'Note body'}).blur();
  await preview.dblclick();await reader.getByRole('button',{name:'Close ticket reader'}).click();await expect(reader).toHaveCount(0);const inspector=page.locator('[data-component="ticket-inspector"]');await expect(inspector.getByRole('textbox',{name:'Ticket details'})).toHaveCount(0);await expect(inspector.getByRole('button',{name:'Edit Ticket details'})).toContainText('The real ticket body.');
  await page.getByRole('button',{name:'Open ticket reader'}).click();reader=page.getByRole('dialog',{name:'Read and edit HS2-DEMO01'});preview=reader.getByRole('button',{name:'Edit Ticket details'});await preview.dblclick();source=reader.getByRole('textbox',{name:'Ticket details'});await source.fill('Saved while the reader closes');await page.screenshot({path:'/private/tmp/hs2-x3gx39-reader-edit-wide.png',fullPage:true});await reader.getByRole('button',{name:'Close ticket reader'}).click();await expect(reader).toHaveCount(0);await expect.poll(()=>patches.some(patch=>patch.details==='Saved while the reader closes')).toBe(true);await expect(inspector.getByRole('textbox',{name:'Ticket details'})).toHaveCount(0);await expect(inspector.getByRole('button',{name:'Edit Ticket details'})).toContainText('Saved while the reader closes');await page.setViewportSize({width:1024,height:600});await page.screenshot({path:'/private/tmp/hs2-x3gx39-reader-close-floor.png',fullPage:true});
});

test('persists separate sidebar and reader heights for each ticket text editor',async({page})=>{
  await mockProject(page);await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();await page.locator('[data-ticket-slug="HS2-DEMO01"]').click();const inspector=page.locator('[data-region-id="app-inspector"]'),resize=async(locator:Locator,height:number)=>{await locator.evaluate((node,value)=>{const textarea=node as HTMLTextAreaElement;textarea.style.height=`${value}px`;textarea.dispatchEvent(new PointerEvent('pointerup',{bubbles:true}));textarea.dispatchEvent(new Event('input',{bubbles:true}))},height);await expect.poll(()=>locator.evaluate(node=>Math.round(node.getBoundingClientRect().height))).toBe(height)};
  await inspector.getByRole('button',{name:'Edit Ticket details'}).dblclick();await resize(inspector.getByRole('textbox',{name:'Ticket details'}),132);await inspector.getByRole('button',{name:'Open ticket reader'}).click();const reader=page.getByRole('dialog',{name:/Read and edit HS2-DEMO01/});await reader.getByRole('button',{name:'Edit Ticket details'}).dblclick();await resize(reader.getByRole('textbox',{name:'Ticket details'}),222);await reader.getByRole('button',{name:'Block ticket'}).click();await resize(reader.getByRole('textbox',{name:'Blocked reason'}),144);await reader.getByText('Editable note with').dblclick();await resize(reader.getByRole('textbox',{name:'Note body'}),188);
  await expect.poll(()=>page.evaluate(()=>Object.fromEntries(['details.sidebar','details.reader','blocked-reason.reader','note.reader'].map(key=>[key,localStorage.getItem(`hotsheet.ticket-editor-height.${key}`)])))).toEqual({'details.sidebar':'132','details.reader':'222','blocked-reason.reader':'144','note.reader':'188'});
});

test('keeps backlog and archived tickets out of the active Queue',async({page})=>{
  await mockProject(page);await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();
  await expect(page.getByRole('heading',{name:'Queue'})).toBeVisible();
  await expect(page.locator('[data-project-dialog]')).not.toBeVisible();
  await expect(page.getByText('Use real project tickets')).toBeVisible();
  await expect(page.getByText('Deferred backlog ticket')).toHaveCount(0);await expect(page.getByText('Archived ticket')).toHaveCount(0);
  await page.getByRole('button',{name:/Backlog/}).click();const backlog=page.locator('[data-ticket-slug="HS2-BACK01"]'),menu=page.getByRole('menu',{name:'Ticket actions'});await expect(backlog).toBeVisible();await backlog.click({button:'right'});await expect(menu.locator('[data-context-action="Move to Backlog"]')).toHaveAttribute('disabled','');await expect(menu.locator('[data-context-action="Archive ticket"]')).not.toHaveAttribute('disabled','');await page.keyboard.press('Escape');await expect(page.getByText('Use real project tickets')).toHaveCount(0);await page.getByRole('button',{name:'New ticket…'}).click();await page.getByRole('textbox',{name:'Ticket title'}).fill('Created directly in backlog');await page.getByRole('button',{name:'Create ticket'}).click();const created=page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-NEW001"]');await expect(created).toBeVisible();await expect(created).toHaveAttribute('data-status','backlog');
  await page.getByRole('button',{name:/Archive/}).click();for(const slug of ['HS2-ARCH01','HS2-DEL001','HS2-MOVED1']){const archived=page.locator(`[data-ticket-slug="${slug}"]`);await expect(archived).toBeVisible();await archived.click({button:'right'});await expect(menu.locator('[data-context-action="Archive ticket"]')).toHaveAttribute('disabled','');await expect(menu.locator('[data-context-action="Move to Backlog"]')).not.toHaveAttribute('disabled','');await page.keyboard.press('Escape')}await expect(page.getByText('Deferred backlog ticket')).toHaveCount(0);await expect(page.locator('[data-component="quick-ticket-composer"]')).toHaveCount(0);
});

test('searches indexed ticket details and notes without discarding the full project list',async({page})=>{
  const searchRequests:string[]=[];await mockProject(page);page.on('request',request=>{const url=new URL(request.url());if(url.pathname.endsWith('/tickets')&&url.searchParams.has('text'))searchRequests.push(url.searchParams.get('text')!)});await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();
  await page.getByRole('button',{name:'Search tickets'}).click();await page.getByRole('textbox',{name:'Search tickets'}).fill('QQRY00');
  await expect(page.locator('[data-ticket-slug="HS2-QQRY00"]')).toBeVisible();await expect(page.locator('[data-ticket-slug="HS2-SHG7YS"]')).toBeVisible();await expect(page.locator('[data-ticket-slug="HS2-DEMO01"]')).toHaveCount(0);await expect.poll(()=>searchRequests).toContain('QQRY00');
  await page.getByRole('button',{name:'Clear search'}).click();await expect(page.locator('[data-ticket-slug="HS2-DEMO01"]')).toBeVisible();await expect(page.locator('[data-ticket-slug="HS2-SHG7YS"]')).toBeVisible();
  const pending=page.waitForRequest(request=>new URL(request.url()).searchParams.get('text')==='QQRY00');await page.getByRole('textbox',{name:'Search tickets'}).fill('QQRY00');await pending;await page.getByRole('button',{name:'Clear search'}).click();await page.waitForTimeout(200);await expect(page.locator('[data-ticket-slug="HS2-DEMO01"]')).toBeVisible();
});

test('derives board columns from the selected view and merges Verified by project setting',async({page})=>{
  await mockProject(page);await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();
  await page.getByLabel('Columns view').click();
  const board=page.locator('.ticket-board');
  await expect(board.locator('.ticket-board-column')).toHaveCount(4);
  await expect(board.locator('.ticket-board-column__title')).toHaveText(['Not Started','Started','Completed','Verified']);
  await expect(board.locator('[data-column-id="completed"]')).toContainText('Completed ticket');
  await expect(board.locator('[data-column-id="verified"]')).toContainText('Verified ticket');

  await page.getByLabel('Settings view').click();
  await page.getByRole('button',{name:'Column view'}).click();
  await page.getByLabel('Hide Verified column').check();
  await expect.poll(()=>page.evaluate(()=>localStorage.getItem('hotsheet.project.demo-checkout.hide-verified-column'))).toBe('true');
  await page.getByLabel('Columns view').click();
  await expect(board.locator('.ticket-board-column__title')).toHaveText(['Not Started','Started','Completed']);
  await expect(board.locator('[data-column-id="completed"]')).toContainText('Completed ticket');
  await expect(board.locator('[data-column-id="completed"]')).toContainText('Verified ticket');

  await page.getByLabel('Settings view').click();
  await page.getByRole('button',{name:'Column view'}).click();
  await page.getByLabel('Hide Verified column').uncheck();
  await page.getByLabel('Columns view').click();
  await expect(board.locator('.ticket-board-column__title')).toHaveText(['Not Started','Started','Completed','Verified']);

  await page.getByRole('button',{name:/Backlog/}).click();
  await expect(board.locator('.ticket-board-column__title')).toHaveText(['Backlog']);
  await expect(board.locator('[data-column-id="backlog"]')).toContainText('Deferred backlog ticket');
  const backlogCard=board.locator('[data-column-id="backlog"] [data-component="ticket-list-row"]');expect(await backlogCard.evaluate(node=>parseFloat(getComputedStyle(node).borderRadius))).toBeGreaterThan(0);await expect(backlogCard).toHaveCSS('border-style','solid');await page.screenshot({path:'/private/tmp/hs2-rzd9d4-backlog-column.png',fullPage:true});
  await page.getByRole('button',{name:/Archive/}).click();
  await expect(board.locator('.ticket-board-column__title')).toHaveText(['Archive']);
  await expect(board.locator('[data-column-id="archive"]')).toContainText('Archived ticket');
  const archiveCards=board.locator('[data-column-id="archive"] [data-component="ticket-list-row"]');await expect(archiveCards).toHaveCount(3);const archiveCard=archiveCards.first();expect(await archiveCard.evaluate(node=>parseFloat(getComputedStyle(node).borderRadius))).toBeGreaterThan(0);await expect(archiveCard).toHaveCSS('border-style','solid');await page.screenshot({path:'/private/tmp/hs2-rzd9d4-archive-column.png',fullPage:true});
});

test('matches HS1 multi-selection semantics and selected outlines in list and column views',async({page})=>{
  await mockProject(page);await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();
  const inspector=page.getByRole('complementary',{name:'Ticket inspector'}),first=page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-DEMO01"]'),second=page.locator('[data-component="ticket-list-row"]').nth(1);await expect(inspector).toContainText('Select a ticket to see and edit its details');await inspector.getByRole('button',{name:'Hide ticket inspector'}).click();await page.getByRole('button',{name:'Show ticket inspector'}).click();await expect(inspector).toBeVisible();await first.click();await second.click({modifiers:['Meta']});await expect(inspector).toContainText('2 items selected — use batch actions to edit them together');await page.screenshot({path:'/private/tmp/hotsheet-multi-selection-placeholder.png'});await expect(first).toHaveAttribute('data-selected','true');await expect(second).toHaveAttribute('data-selected','true');await expect(first).toHaveCSS('border-color','rgb(96, 165, 250)');const selectedBoxes=await Promise.all([first,second].map(locator=>locator.boundingBox()));expect(Math.abs(selectedBoxes[0]!.y+selectedBoxes[0]!.height-selectedBoxes[1]!.y)).toBe(1);
  await page.getByLabel('Columns view').click();const started=page.locator('[data-column-id="started"]');const firstStarted=started.locator('[data-ticket-slug="HS2-DEMO01"]'),thirdStarted=started.locator('[data-ticket-slug="HS2-START03"]');await firstStarted.click();await thirdStarted.click({modifiers:['Shift']});await expect(started.locator('[data-selected="true"]')).toHaveCount(3);
  const startedCount=await started.locator('[data-component="ticket-list-row"]').count();await started.getByRole('button',{name:'Select all Started tickets'}).click();await expect(started.locator('[data-selected="true"]')).toHaveCount(startedCount);await expect(inspector).toContainText(`${startedCount} items selected`);
  const crossColumn=page.locator('[data-column-id="not-started"] [data-ticket-slug="HS2-NEXT01"]');await crossColumn.click({modifiers:['Shift']});await expect(page.locator('.ticket-board [data-selected="true"]')).toHaveCount(1);await expect(crossColumn).toHaveAttribute('data-selected','true');await expect(crossColumn).toHaveCSS('border-color','rgb(96, 165, 250)');await page.screenshot({path:'/private/tmp/hotsheet-real-selection.png'});
  await page.locator('[data-column-id="not-started"] .ticket-board-column__tickets').click({position:{x:240,y:300}});await expect(page.locator('.ticket-board [data-selected="true"]')).toHaveCount(0);await expect(inspector).toContainText('Select a ticket to see and edit its details');await page.screenshot({path:'/private/tmp/hotsheet-zero-selection-placeholder.png'});
});

test('styles completed and verified titles consistently in list and column rows',async({page})=>{
  await mockProject(page);await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();
  const completed=page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-DONE01"] strong');await expect(completed).toHaveCSS('text-decoration-line','line-through');const quietColor=await completed.evaluate(node=>{const probe=document.createElement('span');probe.style.color='var(--wa-color-neutral-on-quiet)';node.append(probe);const color=getComputedStyle(probe).color;probe.remove();return color});await expect(completed).toHaveCSS('color',quietColor);
  await page.getByLabel('Columns view').click();const verified=page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-VERIFY01"] strong');await expect(verified).toHaveCSS('text-decoration-line','line-through');await expect(verified).toHaveCSS('color',quietColor);
});

test('offers Up Next only on active tickets across rows and inspector',async({page})=>{
  await mockProject(page);await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();
  await expect(page.locator('[data-ticket-slug="HS2-NEXT01"] [data-action="toggle-row-up-next"]')).toBeVisible();await expect(page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-DONE01"] [data-action="toggle-row-up-next"]')).toHaveCount(0);
  await page.getByText('Completed ticket',{exact:true}).click();await expect(page.locator('[data-component="ticket-inspector"] [data-action="toggle-inspector-up-next"]')).toHaveCount(0);await page.locator('[data-action="select-view"][data-item-id="archive"]').click();await expect(page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-ARCH01"] [data-action="toggle-row-up-next"]')).toHaveCount(0);
});

test('offers completed verification actions only for one completed ticket',async({page})=>{
  const patches=await mockProject(page,true,false,0,750);await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();const completed=page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-DONE01"]'),verified=page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-VERIFY01"]'),menu=page.getByRole('menu',{name:'Ticket actions'});
  await completed.click({button:'right'});await expect(menu).toBeVisible();await expect(menu.getByRole('menuitem',{name:'Verified',exact:true})).toBeVisible();await expect(menu.getByRole('menuitem',{name:'Not Working…',exact:true})).toBeVisible();await expect(menu.getByText('Toggle Up Next')).toHaveCount(0);await menu.getByRole('menuitem',{name:'Verified',exact:true}).click();await expect.poll(()=>patches.some(patch=>patch.status==='verified')).toBe(true);await expect(completed).toContainText('Verified');await expect(menu).toHaveCount(0);
  await verified.click();await expect(verified).toHaveAttribute('data-selected','true');await completed.click({modifiers:['Meta']});await expect(page.locator('[data-component="ticket-list-row"][data-selected="true"]')).toHaveCount(2);await expect(completed).toHaveAttribute('data-selected','true');await verified.click({button:'right'});await expect(menu).toBeVisible();await expect(menu.getByRole('menuitem',{name:'Verified',exact:true})).toHaveCount(0);await expect(menu.getByRole('menuitem',{name:'Not Working…',exact:true})).toHaveCount(0);await expect(menu.getByText('Toggle Up Next')).toHaveCount(0);
});

test('reports a completed ticket with note and evidence through one atomic provider operation',async({page})=>{
  const patches=await mockProject(page);const requests:string[]=[];page.on('request',request=>{if(request.method()==='POST'&&new URL(request.url()).pathname.endsWith('/not-working'))requests.push(request.postData()??'')});await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();const completed=page.locator('[data-ticket-slug="HS2-DONE01"]');await completed.click({button:'right'});await page.locator('[data-context-action="Report not working"]').click();const dialog=page.getByRole('dialog',{name:'Not Working — HS2-DONE01'}),note=dialog.getByRole('textbox',{name:'What’s wrong?'});await expect(note).toBeFocused();await note.fill('The fix regressed after restart.');const input=dialog.getByLabel('Browse evidence attachments');await input.setInputFiles({name:'proof ünicode.png',mimeType:'image/png',buffer:Buffer.from('png')});await dialog.getByRole('button',{name:'Report Not Working'}).click();await expect(dialog).toHaveCount(0);await expect.poll(()=>patches.some(patch=>patch.operation==='not-working'&&patch.status==='not_started'&&patch.up_next===true)).toBe(true);expect(requests).toHaveLength(1);expect(requests[0]).toContain('The fix regressed after restart.');expect(requests[0]).toContain('proof ünicode.png');await expect(page.locator('[data-ticket-slug="HS2-DONE01"] [data-action="toggle-row-up-next"]')).toHaveAttribute('aria-label','Remove from Up Next');await page.getByRole('button',{name:'Timeline'}).click();const timeline=page.locator('[data-component="ticket-timeline"]');await expect(timeline).toContainText('Brian reported as not working');await expect(timeline).toContainText('The fix regressed after restart.');
});

test('keeps a failed atomic Not Working report open without compensating requests',async({page})=>{
  await mockProject(page);const deletes:string[]=[];page.on('request',request=>{if(request.method()==='DELETE')deletes.push(new URL(request.url()).pathname)});await page.route('**/not-working',route=>route.fulfill({status:500,json:{error:'atomic report failed'}}));await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();await page.locator('[data-ticket-slug="HS2-DONE01"]').click({button:'right'});await page.getByRole('menu',{name:'Ticket actions'}).getByRole('menuitem',{name:'Not Working…',exact:true}).click();const dialog=page.getByRole('dialog',{name:'Not Working — HS2-DONE01'});await dialog.getByLabel('Browse evidence attachments').setInputFiles({name:'failure.txt',mimeType:'text/plain',buffer:Buffer.from('failure')});await dialog.getByRole('button',{name:'Report Not Working'}).click();await expect(dialog.getByRole('textbox',{name:'What’s wrong?'})).toBeVisible();await expect(dialog.getByRole('alert')).toContainText('atomic report failed');await expect(dialog.getByText('failure.txt')).toBeVisible();expect(deletes).toEqual([]);await expect(dialog.getByRole('button',{name:'Report Not Working'})).toBeEnabled();
});

test('opens the matching ticket reader on row double-click in list and columns',async({page})=>{
  await mockProject(page);await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();await page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-DEMO01"]').dblclick();await expect(page.getByRole('dialog',{name:'Read and edit HS2-DEMO01'})).toBeVisible();await page.getByRole('button',{name:'Close ticket reader'}).click();
  await page.getByLabel('Columns view').click();await page.locator('[data-column-id="not-started"] [data-ticket-slug="HS2-NEXT01"]').dblclick();await expect(page.getByRole('dialog',{name:'Read and edit HS2-NEXT01'})).toBeVisible();
});

test('opens shared Markdown links safely in new tabs across the real inspector and reader',async({page})=>{
  await mockProject(page);await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();await page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-DEMO01"]').click();
  const inspector=page.locator('[data-component="ticket-inspector"]'),projectGuide=inspector.getByRole('link',{name:'Project guide'});await expect(projectGuide).toHaveAttribute('target','_blank');await expect(projectGuide).toHaveAttribute('rel','noopener noreferrer');
  const runbook=inspector.getByRole('link',{name:'runbook'});await expect(runbook).toHaveAttribute('target','_blank');await expect(runbook).toHaveAttribute('rel','noopener noreferrer');
  await page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-DEMO01"]').dblclick();const reader=page.getByRole('dialog',{name:'Read and edit HS2-DEMO01'});await expect(reader.getByRole('link',{name:'Project guide'})).toHaveAttribute('target','_blank');await expect(reader.getByRole('link',{name:'runbook'})).toHaveAttribute('rel','noopener noreferrer');
});

test('ships TicketRow context-menu behavior through real list and board compositions',async({page})=>{
  const mutations:string[]=[];page.on('request',request=>{if(['PATCH','POST'].includes(request.method()))mutations.push(new URL(request.url()).pathname)});const patches=await mockProject(page);await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();const first=page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-DEMO01"]'),second=page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-START02"]'),menu=page.getByRole('menu',{name:'Ticket actions'});await first.click();await second.click({modifiers:['Meta']});await first.click({button:'right'});await expect(menu).toBeVisible();await expect(page.locator('[data-component="ticket-list-row"][data-selected="true"]')).toHaveCount(2);await menu.getByText('Toggle Up Next').click();await expect.poll(()=>patches.filter(patch=>patch.up_next===false).length).toBe(2);expect(mutations.filter(path=>path.endsWith('/batch'))).toHaveLength(1);expect(mutations.filter(path=>path.includes('/tickets/'))).toHaveLength(0);
  const choose=async(field:'category'|'priority'|'status',value:string)=>{await first.click({button:'right'});await menu.locator(`wa-dropdown-item:not([slot="submenu"])`,{hasText:`Change ${field}`}).hover();const option=menu.locator(`[data-context-field="${field}"][data-context-value="${value}"]`);await expect(option).toBeVisible();if(field==='category'){await page.waitForTimeout(200);await page.screenshot({path:'/private/tmp/hotsheet-real-context-submenu.png'})}await option.click();await expect(page.locator('[data-component="ticket-list-row"][data-selected="true"]')).toHaveCount(2);await expect.poll(()=>patches.filter(patch=>patch[field]===value).length).toBe(2)};
  // "Open ticket" is hidden while multiple tickets are selected (HS2-XRENF2), so collapse to a
  // single selection before opening the reader from the menu.
  await choose('category','bug');await choose('priority','low');await choose('status','verified');
  expect(patches.filter(patch=>patch.category==='bug'||patch.priority==='normal'||patch.status==='verified').every(patch=>typeof patch.expected_token==='string')).toBe(true);
  await first.click();await first.click({button:'right'});await menu.getByText('Open ticket').click();await expect(page.getByRole('dialog',{name:/Read and edit HS2-DEMO01/})).toBeVisible();await page.getByRole('button',{name:'Close ticket reader'}).click();
  await page.getByLabel('Columns view').click();const boardRow=page.locator('[data-column-id="verified"] [data-ticket-slug="HS2-DEMO01"]');await boardRow.click({button:'right'});await expect(menu).toBeVisible();await page.screenshot({path:'/private/tmp/hotsheet-real-context-menu.png'});await page.keyboard.press('Escape');await expect(menu).toHaveCount(0);
});

test('adds and removes tags and confirms deletion for a real multi-selection',async({page})=>{
  const patches=await mockProject(page),mutationRequests:string[]=[];page.on('request',request=>{if(request.method()!=='GET')mutationRequests.push(`${request.method()} ${new URL(request.url()).pathname}`)});await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();const first=page.locator('[data-ticket-slug="HS2-DEMO01"]'),second=page.locator('[data-ticket-slug="HS2-START02"]'),menu=page.getByRole('menu',{name:'Ticket actions'});const selectBoth=async()=>{await first.click();await second.click({modifiers:['Meta']});await expect(page.locator('[data-action="select-ticket-row"][data-selected="true"]')).toHaveCount(2)};
  await selectBoth();await first.click({button:'right'});await menu.locator('[data-context-action="Add tag"]').click();let dialog=page.locator('[data-component="bulk-tag-dialog"]');await expect(dialog).toContainText('Add tag — 2 selected');await page.getByRole('textbox',{name:'Tag to add *'}).fill('regression');await dialog.getByRole('button',{name:'Add tag'}).click();await expect.poll(()=>patches.filter(patch=>Array.isArray(patch.tags)&&(patch.tags as string[]).includes('regression')).length).toBe(2);
  await selectBoth();await first.click({button:'right'});await menu.locator('[data-context-action="Remove tag"]').click();dialog=page.locator('[data-component="bulk-tag-dialog"]');await expect(dialog).toContainText('Remove tag — 2 selected');await dialog.getByRole('button',{name:'client'}).click();await dialog.getByRole('button',{name:'Remove tag'}).click();await expect.poll(()=>patches.filter(patch=>Array.isArray(patch.tags)&&!(patch.tags as string[]).includes('client')).length).toBe(2);
  await selectBoth();await first.click({button:'right'});await menu.locator('[data-context-action="Delete ticket"]').click();const deletion=page.locator('[data-component="bulk-delete-dialog"]');await expect(deletion).toContainText('Delete 2 tickets?');await page.waitForTimeout(250);await page.screenshot({path:'/private/tmp/hs2-x7vkyj-bulk-delete-wide.png'});await page.setViewportSize({width:390,height:844});await page.waitForTimeout(250);await page.screenshot({path:'/private/tmp/hs2-x7vkyj-bulk-delete-narrow.png',fullPage:true});await deletion.getByRole('button',{name:'Delete 2 tickets'}).click();await expect.poll(()=>patches.filter(patch=>patch.status==='deleted').length).toBe(2);await expect(first).toHaveCount(0);expect(patches.filter(patch=>Array.isArray(patch.tags)||patch.status==='deleted').every(patch=>typeof patch.expected_token==='string')).toBe(true);expect(mutationRequests.filter(value=>value.endsWith('/batch'))).toHaveLength(3);expect(mutationRequests.filter(value=>value.includes('/tickets/'))).toEqual([]);
});

test('dismisses ticket context menus on every true outside pointerdown',async({page})=>{
  await mockProject(page);await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();
  const first=page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-DEMO01"]'),second=page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-START02"]'),menu=page.getByRole('menu',{name:'Ticket actions'});
  await first.click({button:'right'});await expect(menu).toBeVisible();
  await menu.locator('wa-dropdown-item',{hasText:'Change status'}).dispatchEvent('pointerdown');await expect(menu).toBeVisible();
  await page.getByRole('button',{name:'Search tickets'}).click();await expect(menu).toHaveCount(0);
  await first.click({button:'right'});await expect(menu).toBeVisible();await second.dispatchEvent('pointerdown');await expect(menu).toHaveCount(0);await second.click();
  await first.click({button:'right'});await expect(menu).toBeVisible();await page.keyboard.press('Escape');await expect(menu).toHaveCount(0);
});

test('undoes, redoes, copies, pastes, and drags ticket mutations through the real shell',async({page})=>{
  const patches=await mockProject(page);await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();
  const ticket=page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-DEMO01"]');await ticket.click();
  const nextPatch=()=>page.waitForResponse(response=>response.request().method()==='PATCH'&&new URL(response.url()).pathname.endsWith('/tickets/01'));
  await page.locator('wa-select[name="inspector-status"]').click();let response=nextPatch();await page.locator('wa-select[name="inspector-status"] wa-option[value="completed"]').click();await response;
  await expect(page.locator('[data-component="ticket-inspector"] [data-component="status-badge"]')).toContainText('Completed');await expect.poll(()=>patches.filter(patch=>patch.status==='completed').length).toBe(1);
  await page.locator('.app-shell__work-area').focus();response=nextPatch();await page.keyboard.press('Control+z');await response;await expect(page.locator('[data-component="ticket-inspector"] [data-component="status-badge"]')).toContainText('Started');await expect.poll(()=>patches.filter(patch=>patch.status==='started').length).toBe(1);await page.waitForTimeout(0);
  await page.locator('.app-shell__work-area').focus();response=nextPatch();await page.keyboard.press('Control+Shift+z');await response;await expect(page.locator('[data-component="ticket-inspector"] [data-component="status-badge"]')).toContainText('Completed');await expect.poll(()=>patches.filter(patch=>patch.status==='completed').length).toBe(2);
  await ticket.focus();await page.keyboard.press('Control+c');await page.locator('.app-shell__work-area').focus();await page.keyboard.press('Control+v');await expect(page.getByText('Use real project tickets (Copy)')).toBeVisible();
  await ticket.evaluate(node=>{const transfer=new DataTransfer();node.dispatchEvent(new DragEvent('dragstart',{bubbles:true,dataTransfer:transfer}));document.querySelector<HTMLElement>('[data-ticket-drop-status="backlog"]')!.dispatchEvent(new DragEvent('dragover',{bubbles:true,cancelable:true,dataTransfer:transfer}))});
  await expect(page.locator('[data-ticket-drop-status="backlog"]')).toHaveAttribute('data-dragging-ticket','true');
  await page.locator('[data-ticket-drop-status="backlog"]').dispatchEvent('drop');
  await page.locator('[data-ticket-drop-status="backlog"]').click();await expect(page.locator('[data-component="ticket-list-row"]',{hasText:'Use real project tickets'})).toBeVisible();
});

test('drags single and selected tickets across columns, views, and the duplicate target',async({page})=>{
  const mutations:string[]=[];page.on('request',request=>{if(['PATCH','POST'].includes(request.method()))mutations.push(`${request.method()} ${new URL(request.url()).pathname}`)});const patches=await mockProject(page);await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();
  const first=page.locator('[data-ticket-slug="HS2-DEMO01"]'),second=page.locator('[data-ticket-slug="HS2-START02"]'),dragTo=async(source:Locator,target:string)=>{await source.evaluate((node,selector)=>{const transfer=new DataTransfer();node.dispatchEvent(new DragEvent('dragstart',{bubbles:true,dataTransfer:transfer}));document.querySelector<HTMLElement>(selector)!.dispatchEvent(new DragEvent('dragover',{bubbles:true,cancelable:true,dataTransfer:transfer}))},target);await expect(page.locator(target)).toHaveAttribute('data-dragging-ticket','true');await page.locator(target).dispatchEvent('drop')};
  await first.click();await second.click({modifiers:['Meta']});const writesBefore=mutations.length;await dragTo(first,'[data-ticket-drop-status="not_started"][data-item-id="all"]');expect(mutations).toHaveLength(writesBefore);
  await dragTo(first,'[data-ticket-drop-status="backlog"]');await expect.poll(()=>patches.filter(patch=>patch.status==='backlog').length).toBe(2);expect(mutations.filter(value=>value.endsWith('/batch'))).toHaveLength(1);
  await page.locator('[data-ticket-drop-status="backlog"]').click();const backlogFirst=page.locator('[data-ticket-slug="HS2-DEMO01"]');await expect(backlogFirst).toBeVisible();await dragTo(backlogFirst,'[data-ticket-drop-status="not_started"][data-item-id="all"]');await expect.poll(()=>patches.filter(patch=>patch.status==='not_started').length).toBe(2);expect(mutations.filter(value=>value.endsWith('/batch'))).toHaveLength(2);
  await page.locator('[data-ticket-drop-status="not_started"][data-item-id="all"]').click();await page.getByLabel('Columns view').click();const boardFirst=page.locator('[data-column-id="not-started"] [data-ticket-slug="HS2-DEMO01"]');await boardFirst.click();await dragTo(boardFirst,'[data-column-id="completed"]');await expect.poll(()=>patches.filter(patch=>patch.status==='completed').length).toBe(1);await expect(page.locator('[data-column-id="completed"] [data-ticket-slug="HS2-DEMO01"]')).toBeVisible();
  await page.getByLabel('List view').click();const duplicateFirst=page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-DEMO01"]'),duplicateSecond=page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-START02"]');await duplicateFirst.click();await duplicateSecond.click({modifiers:['Meta']});const createsBefore=mutations.filter(value=>value.endsWith('/tickets')).length;await dragTo(duplicateFirst,'[data-ticket-drop-action="duplicate"]');await expect.poll(()=>mutations.filter(value=>value.endsWith('/tickets')).length).toBe(createsBefore+2);await expect(page.locator('.app-toast')).toContainText('2 tickets copied to demo.');await page.screenshot({path:'/private/tmp/hs2-w2743r-ticket-drag-targets.png',fullPage:true});
});

test('drops selected tickets on another project tab to copy them there',async({page})=>{
  const creates:string[]=[];await mockProject(page);await page.route('**/__hotsheet/projects/open',route=>{const root=route.request().postDataJSON().root as string;if(root==='/work/other')return route.fulfill({status:201,json:{...project,id:'other-checkout',root,name:'other',apiPath:'/__hotsheet/project-api/other-checkout'}});return route.fulfill({status:201,json:project})});page.on('request',request=>{const path=new URL(request.url()).pathname;if(request.method()==='POST'&&path.includes('/checkouts/other-checkout/tickets'))creates.push(path)});
  await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();await page.getByRole('button',{name:'Add project'}).click();await page.locator('wa-input[name="project-root"]').evaluate((node:HTMLElement&{value:string})=>{node.value='/work/other';node.dispatchEvent(new Event('input',{bubbles:true}))});await page.getByRole('button',{name:'Open project',exact:true}).last().click();await page.getByRole('tab',{name:'demo'}).click();const first=page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-DEMO01"]'),second=page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-START02"]');await expect(first).toBeVisible();await first.click();await second.click({modifiers:['Meta']});
  await first.evaluate(node=>{const transfer=new DataTransfer();node.dispatchEvent(new DragEvent('dragstart',{bubbles:true,dataTransfer:transfer}));document.querySelector<HTMLElement>('[data-ticket-drop-project="other-checkout"]')!.dispatchEvent(new DragEvent('dragover',{bubbles:true,cancelable:true,dataTransfer:transfer}))});const destination=page.locator('[data-ticket-drop-project="other-checkout"]');await expect(destination).toHaveAttribute('data-dragging-ticket','true');await destination.dispatchEvent('drop');await expect.poll(()=>creates.length).toBe(2);await expect(page.locator('.app-toast')).toContainText('2 tickets copied to other.');await expect(page.getByRole('tab',{name:'demo'})).toHaveAttribute('aria-selected','true');
});

test('scopes ticket clipboard shortcuts to the focused work area and preserves native text copy and paste',async({page,context})=>{
  await context.grantPermissions(['clipboard-read','clipboard-write']);const creates:string[]=[];page.on('request',request=>{const path=new URL(request.url()).pathname;if(request.method()==='POST'&&path.endsWith('/tickets'))creates.push(path)});await mockProject(page);await page.goto('/');await page.evaluate(()=>{(window as typeof window&{shortcutDefaults?:boolean[]}).shortcutDefaults=[];document.addEventListener('keydown',event=>{if((event.ctrlKey||event.metaKey)&&['c','v'].includes(event.key.toLowerCase()))queueMicrotask(()=>{(window as typeof window&{shortcutDefaults:boolean[]}).shortcutDefaults.push(event.defaultPrevented)})})});await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();
  const workArea=page.locator('.app-shell__work-area'),ticket=page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-DEMO01"]');await ticket.click();await expect(workArea).toHaveCSS('outline-width','2px');await expect(workArea).toHaveCSS('outline-color','rgb(59, 130, 246)');
  await page.keyboard.press('Control+c');await expect.poll(()=>page.evaluate(()=>(window as typeof window&{shortcutDefaults:boolean[]}).shortcutDefaults.at(-1))).toBe(true);
  const inspectorSlug=page.locator('[data-component="ticket-inspector"] [data-component="toolbar-text"]');await inspectorSlug.click();await inspectorSlug.evaluate(node=>{const range=document.createRange();range.selectNodeContents(node);const selection=getSelection()!;selection.removeAllRanges();selection.addRange(range)});await expect(workArea).toHaveCSS('outline-width','0px');await page.keyboard.press('Control+c');await expect.poll(()=>page.evaluate(()=>(window as typeof window&{shortcutDefaults:boolean[]}).shortcutDefaults.at(-1))).toBe(false);expect(creates).toHaveLength(0);
  await page.getByRole('button',{name:'Search tickets'}).click();const search=page.getByRole('textbox',{name:'Search tickets'});await search.focus();await expect(search).toBeFocused();await page.evaluate(()=>navigator.clipboard.writeText('QQRY00'));await page.keyboard.press('Control+v');await expect.poll(()=>page.evaluate(()=>(window as typeof window&{shortcutDefaults:boolean[]}).shortcutDefaults.at(-1))).toBe(false);expect(creates).toHaveLength(0);
  await page.evaluate(()=>getSelection()?.removeAllRanges());await workArea.focus();await expect(workArea).toHaveCSS('outline-width','2px');await page.keyboard.press('Control+v');await expect(page.getByText('Use real project tickets (Copy)')).toBeVisible();expect(creates).toHaveLength(1);
  await page.screenshot({path:'/private/tmp/hs2-rg612c-work-area-focus-wide.png',fullPage:true});await page.setViewportSize({width:390,height:844});await workArea.focus();await expect(workArea).toHaveCSS('outline-width','2px');await page.screenshot({path:'/private/tmp/hs2-rg612c-work-area-focus-narrow.png',fullPage:true});
});

test('preserves attachments and atomically undoes and redoes cut-paste',async({page})=>{
  const copyRequests:unknown[]=[];page.on('request',request=>{if(new URL(request.url()).pathname.endsWith('/provider-attachments/copy'))copyRequests.push(request.postDataJSON())});
  await mockProject(page);await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();
  const source=page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-DEMO01"]');await source.click();await page.getByRole('button',{name:'Attachments'}).click();await expect(page.locator('[data-attachment-id="A1"]')).toBeVisible();await source.focus();await page.keyboard.press('Control+x');await page.locator('.app-shell__work-area').focus();await page.keyboard.press('Control+v');
  await expect(page.getByText('Use real project tickets (Copy)')).toBeVisible();
  await expect.poll(()=>copyRequests.length).toBe(1);
  expect(copyRequests[0]).toEqual({source:{connection_id:'git-local',native_id:'01',attachment_id:'A1'},destination:{connection_id:'git-local',native_id:'02'}});
  await expect(source).toHaveCount(0);
  await page.keyboard.press('Control+z');await expect(source).toBeVisible();await expect(page.getByText('Use real project tickets (Copy)')).toHaveCount(0);
  await page.keyboard.press('Control+Shift+z');await expect(source).toHaveCount(0);await expect(page.getByText('Use real project tickets (Copy)')).toBeVisible();
});

test('shows and resolves cross-project permission notifications with badges and history',async({page})=>{
  await mockProject(page);let pending=[{id:7,connection:'claude-session',tool:'Edit',action:'/work/demo/src/main.ts',always_allow_supported:true}],answer:unknown;
  await page.route('**/connections',route=>route.fulfill({json:[{id:'claude-session',tool:'Claude',project:'/work/demo',role:'main',busy:true}]}));
  await page.route('**/permissions',route=>route.fulfill({json:pending}));
  await page.route('**/permissions/7',route=>{answer=route.request().postDataJSON();pending=[];return route.fulfill({json:{connection:'claude-session',decision:'allow',persisted:true}})});
  await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();
  const popup=page.locator('[data-component="permission-request-popup"]');await expect(popup).toBeVisible();await expect(popup).toContainText('demo');await expect(popup).toContainText('/work/demo/src/main.ts');
  await expect(page.getByRole('button',{name:/Notifications view, 1 pending/})).toBeVisible();await expect(page.locator('[data-component="project-tab"] .project-tab__notification')).toContainText('1');
  const notificationsButton=page.getByRole('button',{name:/Notifications view/});await notificationsButton.click();await expect(notificationsButton).toHaveAttribute('aria-pressed','true');await expect(page.getByRole('button',{name:'List view'})).toHaveAttribute('aria-pressed','false');await expect(page.locator('[data-component="notification-navigation"]')).toBeVisible();await expect(page.getByRole('heading',{name:'Pending'})).toBeVisible();await expect(page.locator('[data-component="notification-center"]')).toBeVisible();await expect(popup).toBeVisible();await expect(page.locator('.notification-inspector-empty')).toBeVisible();await page.screenshot({path:'/private/tmp/hotsheet-permission-shell.png'});await page.setViewportSize({width:1024,height:600});await expect(popup).toBeInViewport();await page.screenshot({path:'/private/tmp/hotsheet-permission-shell-narrow.png'});
  await popup.getByRole('button',{name:'Always Allow'}).click();await expect.poll(()=>answer).toEqual({decision:'allow',scope:'always'});await expect(popup).toHaveCount(0);await expect(page.getByText('allowed this kind of request')).toHaveCount(0);await page.getByRole('button',{name:/Last 24 Hours/}).click();await expect(page.getByRole('heading',{name:'Last 24 Hours'})).toBeVisible();await expect(page.getByText('allowed this kind of request')).toBeVisible();await page.getByRole('button',{name:/Last 7 Days/}).click();await expect(page.getByRole('heading',{name:'Last 7 Days'})).toBeVisible();await expect(page.getByText('allowed this kind of request')).toBeVisible();
});

test('counts permission automation only while its popup is visible',async({page})=>{
  await page.clock.install();await mockProject(page);let pending=[{id:8,connection:'codex-session',tool:'item/commandExecution/requestApproval',action:'npm test',always_allow_supported:true}],answers=0;
  await page.route('**/connections',route=>route.fulfill({json:[{id:'codex-session',tool:'Codex',project:'/work/demo',role:'worker',busy:true}]}));
  await page.route('**/permissions',route=>route.fulfill({json:pending}));
  await page.route('**/permissions/8',route=>{answers+=1;pending=[];return route.fulfill({json:{connection:'codex-session',decision:'allow',persisted:false}})});
  await page.goto('/');await page.evaluate(()=>{localStorage.setItem('hotsheet.project.demo-checkout.permission-automation',JSON.stringify({action:'allow',delayMs:15_000}))});await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();
  const popup=page.locator('[data-component="permission-request-popup"]');await expect(popup).toContainText('Auto-allow in');await page.clock.fastForward(10_000);await expect(popup).toContainText('0:05');await page.getByLabel('Settings view').click();await page.getByRole('button',{name:'Permissions'}).click();const automation=page.locator('wa-select[name="permission-automation-action"]');await automation.click();await expect(automation).toHaveJSProperty('open',true);await resetRenderMetrics(page);await page.clock.fastForward(1_000);await expect(popup).toContainText('0:04');await expect(automation).toHaveJSProperty('open',true);expect((await renderMetrics(page))?.passes).toBe(0);await page.keyboard.press('Escape');await popup.getByRole('button',{name:'Stop auto-allow countdown'}).click();await expect(popup.locator('.permission-request-card__countdown')).toHaveCount(0);await page.clock.fastForward(30_000);expect(answers).toBe(0);await popup.getByRole('button',{name:'Ignore'}).click();await expect(popup).toHaveCount(0);await page.getByRole('button',{name:/Notifications view/}).click();await page.locator('[data-component="notification-center"]').getByRole('button',{name:'Allow Once'}).click();await expect.poll(()=>answers).toBe(1);
});

test('persists and restores per-project permission automation settings',async({page})=>{
  await mockProject(page);await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();await page.getByLabel('Settings view').click();await page.getByRole('button',{name:'Permissions'}).click();
  const action=page.locator('wa-select[name="permission-automation-action"]'),delay=page.locator('wa-select[name="permission-automation-delay"]');await expect(action).toHaveJSProperty('value','off');await expect(delay).toHaveJSProperty('disabled',true);
  await action.evaluate((node:HTMLElement&{value:string})=>{node.value='deny';node.dispatchEvent(new Event('change',{bubbles:true}))});await expect(delay).toHaveJSProperty('disabled',false);await delay.evaluate((node:HTMLElement&{value:string})=>{node.value='120000';node.dispatchEvent(new Event('change',{bubbles:true}))});
  await expect.poll(()=>page.evaluate(()=>localStorage.getItem('hotsheet.project.demo-checkout.permission-automation'))).toBe('{"action":"deny","delayMs":120000}');await page.getByLabel('List view').click();await page.getByLabel('Settings view').click();await page.getByRole('button',{name:'Permissions'}).click();await expect(action).toHaveJSProperty('value','deny');await expect(delay).toHaveJSProperty('value','120000');
});

test('live project visual review',async({page})=>{
  test.skip(!process.env.HOTSHEET_LIVE_PROJECT,'opt-in local visual review');
  const pageErrors:string[]=[];page.on('pageerror',error=>pageErrors.push(error.message));
  await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();
  await page.locator('wa-input[name="project-root"]').evaluate((node:HTMLElement&{value:string},value)=>node.value=value,process.env.HOTSHEET_LIVE_PROJECT);
  await page.getByRole('button',{name:'Open project',exact:true}).last().click();
  await expect(page.locator('[data-component="ticket-list-row"]').first()).toBeVisible({timeout:15_000});
  const loaded=page.waitForResponse(response=>response.url().includes('/tickets/')&&response.request().method()==='GET');
  await page.locator('[data-component="ticket-list-row"]').first().click();
  expect((await loaded).status()).toBe(200);
  expect(pageErrors).toEqual([]);
  await expect(page.locator('[data-component="ticket-inspector"]')).toBeVisible();
  if(process.env.HOTSHEET_LIVE_ATTACHMENT){
    await page.getByRole('button',{name:'Attachments'}).click();
    const uploaded=page.waitForResponse(response=>response.url().endsWith('/attachments')&&response.request().method()==='POST');
    await page.getByLabel('Browse and add attachments').setInputFiles(process.env.HOTSHEET_LIVE_ATTACHMENT);
    expect((await uploaded).status()).toBe(201);
    const filename=process.env.HOTSHEET_LIVE_ATTACHMENT.split('/').at(-1)!;
    await expect(page.locator('[data-component="ticket-attachments"]')).toContainText(filename);
    await page.getByRole('button',{name:`Remove ${filename}`}).click();
    await expect(page.locator('[data-component="ticket-attachments"]')).not.toContainText(filename);
  }
  await page.screenshot({path:'/private/tmp/hotsheet-real-app.png',fullPage:true});
  await page.setViewportSize({width:900,height:760});
  await expect(page.locator('[data-component="ticket-list-row"]').first()).toBeVisible();
  await page.screenshot({path:'/private/tmp/hotsheet-real-app-narrow.png',fullPage:true});
});

test('preserves workspace scroll position when the ticket context menu opens (HS2-H4MWDB)',async({page})=>{
  const base={connection_id:'git-local',native_id:'01',qualified_id:'git-local:01',id:'01',slug:'HS2-DEMO01',title:'Ticket',category:'feature',priority:'high',status:'started',up_next:false,tags:['client'],blocked_by:[],claim_count:0,created_at:'2026-08-30T00:00:00Z',updated_at:'2026-08-30T01:00:00Z'};
  const many=Array.from({length:40},(_,i)=>({...base,id:String(100+i),native_id:String(100+i),qualified_id:`git-local:${100+i}`,slug:`HS2-ROW${String(i).padStart(2,'0')}`,title:`Scrollable ticket number ${i}`}));
  const proj={id:'demo-checkout',root:'/work/demo',name:'demo',stores:['/work/demo.hs2'],apiPath:'/__hotsheet/project-api/demo-checkout'};
  await page.route('**/*',async route=>{const req=route.request(),path=new URL(req.url()).pathname;
    if(path==='/__hotsheet/projects/open')return route.fulfill({status:201,json:proj});
    if(path.endsWith('/providers'))return route.fulfill({json:[{connection_id:'git-local',provider:'git',display_name:'Hot Sheet git',locator:'/tickets',default:true,capabilities:{create:true,update:true,close:true,notes:true,note_edit:true,note_delete:true,attachments:true,assignment:true,review_requests:true,dependencies:true,up_next:true,close_reasons:true,claims:true,atomic_batch:true,not_working_report:true,offline_mutation:true,history:true,watch:true,provider_idempotency:true,query_fields:[]}}]});
    if(path.endsWith('/permissions')&&req.method()==='GET')return route.fulfill({json:[]});
    if(path.endsWith('/connections')&&req.method()==='GET')return route.fulfill({json:[]});
    if(path.endsWith('/repository/status'))return route.fulfill({json:{branch:'main',ahead:0,behind:0,staged:0,unstaged:0,untracked:0,conflicted:0,clean:true}});
    if(path.endsWith('/tickets')&&req.method()==='GET')return route.fulfill({json:many});
    if(path.includes('/tickets/')&&req.method()==='GET'){const id=path.split('/').pop();const t=many.find(r=>r.id===id)||many[0];return route.fulfill({json:{store:'git-local',...t,details:'body',blocked_reason:null,notes:[],attachments:[]}})}
    return route.continue();});
  await page.setViewportSize({width:1400,height:820});
  await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();
  const menu=page.locator('.ticket-context-menu[role="menu"]');
  await expect(page.locator('[data-action="select-ticket-row"]').first()).toBeVisible();
  // List view: scroll the workspace, then open the context menu on a currently-visible row.
  const list=await page.evaluate(()=>{const ws=document.querySelector('.app-shell__workspace') as HTMLElement;ws.scrollTop=400;const before=ws.scrollTop;const rows=[...document.querySelectorAll('[data-action="select-ticket-row"]')];const r=ws.getBoundingClientRect();const v=rows.find(el=>{const b=el.getBoundingClientRect();return b.top>=r.top+10&&b.bottom<=r.bottom-10}) as HTMLElement;const b=v.getBoundingClientRect();v.dispatchEvent(new MouseEvent('contextmenu',{bubbles:true,cancelable:true,clientX:b.left+30,clientY:b.top+10}));return {before}});
  await expect(menu).toBeVisible();
  expect(await page.evaluate(()=>(document.querySelector('.app-shell__workspace') as HTMLElement).scrollTop)).toBe(list.before);
  await page.keyboard.press('Escape');
  await expect(menu).toBeHidden();
  // Board view: scroll a column's ticket list, then open the context menu on a visible row in it.
  await page.getByRole('button',{name:'Columns view'}).click();
  await expect(page.locator('.ticket-board-column__tickets').first()).toBeVisible();
  const board=await page.evaluate(()=>{const col=[...document.querySelectorAll('.ticket-board-column__tickets')].find(c=>c.scrollHeight>c.clientHeight+50) as HTMLElement;col.scrollTop=300;const before=col.scrollTop;const rows=[...col.querySelectorAll('[data-action="select-ticket-row"]')];const r=col.getBoundingClientRect();const v=rows.find(el=>{const b=el.getBoundingClientRect();return b.top>=r.top+10&&b.bottom<=r.bottom-10}) as HTMLElement;const b=v.getBoundingClientRect();v.dispatchEvent(new MouseEvent('contextmenu',{bubbles:true,cancelable:true,clientX:b.left+30,clientY:b.top+10}));return {before,colId:col.closest('[data-column-id]')!.getAttribute('data-column-id')}});
  await expect(menu).toBeVisible();
  expect(await page.evaluate(id=>{const col=document.querySelector(`[data-column-id="${id}"] .ticket-board-column__tickets`) as HTMLElement;return col.scrollTop},board.colId)).toBe(board.before);
});
