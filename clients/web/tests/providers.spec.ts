import { expect,test } from '@playwright/test';

const project = { id:'demo-checkout', root:'/work/demo', name:'demo', stores:['/work/demo.hs2'], apiPath:'/__hotsheet/project-api/demo-checkout' };
const row = { connection_id:'git-local', native_id:'01', qualified_id:'git-local:01', id:'01', slug:'HS2-DEMO01', title:'Use real project tickets', category:'feature', priority:'high', status:'started', up_next:true, tags:['client'], blocked_by:[], claim_count:0, created_at:'2026-08-30T00:00:00Z', updated_at:'2026-08-30T01:00:00Z' };
const backlogRow = { ...row, native_id:'03', qualified_id:'git-local:03', id:'03', slug:'HS2-BACK01', title:'Deferred backlog ticket', status:'backlog', up_next:false };
const archiveRow = { ...row, native_id:'04', qualified_id:'git-local:04', id:'04', slug:'HS2-ARCH01', title:'Archived ticket', status:'archive', up_next:false };
const notStartedRow = { ...row, native_id:'05', qualified_id:'git-local:05', id:'05', slug:'HS2-NEXT01', title:'Not started ticket', status:'not_started', up_next:false };
const completedRow = { ...row, native_id:'06', qualified_id:'git-local:06', id:'06', slug:'HS2-DONE01', title:'Completed ticket', status:'completed', up_next:false };
const verifiedRow = { ...row, native_id:'07', qualified_id:'git-local:07', id:'07', slug:'HS2-VERIFY01', title:'Verified ticket', status:'verified', up_next:false };
const full = { ...row, details:'The real ticket body.', blocked_reason:null, concurrency_token:'token', notes:[{id:'N1',kind:'activity',created_at:'2026-08-30T00:30:00Z',edited_at:'2026-08-30T00:30:00Z',text:'Connected the client\nLoaded checkout-scoped tickets.'},{id:'N2',kind:'feedback_needed',created_at:'2026-08-30T00:35:00Z',edited_at:'2026-08-30T00:35:00Z',text:'Should this reader preserve the current draft?'},{id:'N3',kind:'regular',created_at:'2026-08-30T00:36:00Z',edited_at:'2026-08-30T00:36:00Z',text:'Editable note'}], attachments:[{id:'A1',filename:'proof.png',created_at:'2026-08-30T00:40:00Z'}] };

async function mockProject(page: import('@playwright/test').Page, canUpdate = true) {
  let rows = [row,backlogRow,archiveRow,notStartedRow,completedRow,verifiedRow];
  let selectedFull = full;
  const patches: Record<string,unknown>[] = [];
  await page.route('**/*', async route => {
    const request=route.request(), url=new URL(request.url()), path=url.pathname;
    if(path==='/__hotsheet/projects/open') return route.fulfill({status:201,json:project});
    if(path.endsWith('/providers')) return route.fulfill({json:[{connection_id:'git-local',provider:'git',display_name:'Hot Sheet git',locator:'/tickets',default:true,capabilities:{create:true,update:canUpdate,close:true,notes:true,note_edit:canUpdate,note_delete:canUpdate,attachments:true,assignment:true,review_requests:true,dependencies:true,up_next:true,close_reasons:true,claims:true,atomic_batch:true,offline_mutation:true,history:true,watch:true,provider_idempotency:true,query_fields:[]}}]});
    if(path.endsWith('/permissions')&&request.method()==='GET')return route.fulfill({json:[]});
    if(path.endsWith('/connections')&&request.method()==='GET')return route.fulfill({json:[]});
    if(path.endsWith('/repository/status')) return route.fulfill({json:{branch:'main',ahead:1,behind:0,staged:0,unstaged:1,untracked:0,conflicted:0,clean:false}});
    if(path.endsWith('/tickets')&&request.method()==='GET') return route.fulfill({json:rows});
    if(path.endsWith('/tickets')&&request.method()==='POST'){const body=request.postDataJSON();const created={...row,id:'02',native_id:'02',slug:'HS2-NEW001',title:body.title,category:body.category,up_next:false};rows=[created,...rows];return route.fulfill({status:201,json:{...created,details:'',notes:[],attachments:[]}})}
    if(path.endsWith('/provider-attachments/copy')&&request.method()==='POST'){const destination=rows.find(item=>item.native_id===request.postDataJSON().destination.native_id)!;return route.fulfill({status:201,json:{...destination,details:'',notes:[],attachments:[{id:'A-COPY',filename:'proof.png',created_at:'2026-08-30T01:15:00Z'}]}})}
    if(path.endsWith('/tickets/01')&&request.method()==='GET') return route.fulfill({json:{store:'git-local',...selectedFull}});
    if(path.endsWith('/tickets/01/attachments')&&request.method()==='POST'){const filename=request.headers()['x-hotsheet-filename']??'attachment';selectedFull={...selectedFull,attachments:[...selectedFull.attachments,{id:`A${selectedFull.attachments.length+1}`,filename,created_at:'2026-08-30T01:10:00Z'}]};return route.fulfill({status:201,json:{store:'git-local',...selectedFull}})}
    if(path.includes('/tickets/01/attachments/')&&request.method()==='GET')return route.fulfill({body:'attachment bytes',headers:{'content-type':'application/octet-stream','x-hotsheet-filename':'proof.png'}});
    if(path.includes('/tickets/01/attachments/')&&request.method()==='DELETE'){const attachmentId=path.split('/').pop();selectedFull={...selectedFull,attachments:selectedFull.attachments.filter(item=>item.id!==attachmentId)};return route.fulfill({json:{store:'git-local',...selectedFull}})}
    if(path.includes('/tickets/01/notes/')&&request.method()==='DELETE'){const noteId=path.split('/').pop();selectedFull={...selectedFull,notes:selectedFull.notes.filter(note=>{return note.id!==noteId})};return route.fulfill({json:{store:'git-local',...selectedFull}})}
    if(path.includes('/tickets/')&&request.method()==='PATCH'){const id=path.split('/').pop(),body=request.postDataJSON();patches.push(body);rows=rows.map(item=>item.id===id?{...item,...body}:item);if(id==='01'){selectedFull={...selectedFull,...body};return route.fulfill({json:{store:'git-local',...selectedFull}})}const changed=rows.find(item=>item.id===id)!;return route.fulfill({json:{store:'git-local',...changed,details:'',notes:[],attachments:[]}})}
    return route.continue();
  });
  return patches;
}

test('translates urgent priority through the canonical server contract',async({page})=>{
  const patches=await mockProject(page);await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();await page.getByText('Use real project tickets').click();
  await page.locator('wa-select[name="inspector-priority"]').evaluate((node:HTMLElement&{value:string})=>{node.value='urgent';node.dispatchEvent(new Event('change',{bubbles:true}))});
  await expect.poll(()=>patches.at(-1)?.priority).toBe('highest');
  await expect(page.locator('[data-component="ticket-inspector"] wa-select[name="inspector-priority"]')).toHaveJSProperty('value','urgent');
  await expect(page.locator('[data-component="ticket-inspector"] wa-select[name="inspector-priority"] .select__icon--selected [data-lucide="chevrons-up"]')).toBeVisible();
});

test('projects Up Next immediately and reconciles without a full project refresh',async({page})=>{
  await mockProject(page);await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();
  const row=page.locator('[data-component="ticket-list-row"]',{hasText:'Not started ticket'});const requests:string[]=[];page.on('request',request=>{requests.push(`${request.method()} ${new URL(request.url()).pathname}`)});
  await page.route('**/tickets/05',async route=>{if(route.request().method()!=='PATCH')return route.continue();await new Promise(resolve=>setTimeout(resolve,250));const body=route.request().postDataJSON();return route.fulfill({json:{store:'git-local',...notStartedRow,...body,details:'',notes:[],attachments:[]}})});
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

  await inspector.getByRole('button',{name:'Block ticket'}).click();const blocked=inspector.getByRole('textbox',{name:'Blocked reason'});await blocked.fill('Waiting for review');
  await expect.poll(()=>patches.some(patch=>patch.blocked_reason==='Waiting for review')).toBe(true);
});

test('creates, cancels, edits, and deletes notes through the shared inspector and reader',async({page})=>{
  const patches=await mockProject(page);await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();await page.getByText('Use real project tickets').click();
  const inspector=page.locator('[data-component="ticket-inspector"]');await inspector.getByRole('button',{name:'Add note'}).first().click();const composer=inspector.locator('[data-component="note-composer"]');await expect(composer.getByRole('textbox',{name:'New note'})).toBeFocused();await composer.getByRole('textbox',{name:'New note'}).fill('Discard me');await composer.getByRole('button',{name:'Cancel'}).click();await expect(composer).toHaveCount(0);
  await inspector.getByRole('button',{name:'Add note'}).first().click();await inspector.getByRole('textbox',{name:'New note'}).fill('Created note');await inspector.getByRole('button',{name:'Add note',exact:true}).last().click();await expect.poll(()=>patches.some(patch=>patch.note==='Created note'&&patch.note_kind==='regular')).toBe(true);
  await inspector.locator('[data-note-id="N3"]').getByRole('button',{name:'Edit note'}).click();const editor=inspector.getByRole('textbox',{name:'Note body'});await editor.fill('Edited lifecycle note');await expect.poll(()=>patches.some(patch=>patch.note_id==='N3'&&patch.note==='Edited lifecycle note')).toBe(true);await editor.blur();await expect(editor).toHaveCount(0);
  await inspector.locator('[data-note-id="N3"]').getByRole('button',{name:'Delete note'}).click();await expect(inspector.locator('[data-note-id="N3"]')).toHaveCount(0);
  await inspector.getByRole('button',{name:'Open ticket reader'}).click();const reader=page.getByRole('dialog');await reader.getByRole('button',{name:'Add note'}).first().click();await expect(reader.locator('[data-component="note-composer"]')).toBeVisible();
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
  await mockProject(page); await page.goto('/');
  await page.getByRole('button',{name:'Open project'}).click();
  await expect(page.locator('wa-input[name="project-root"]')).toHaveJSProperty('value','/Users/westphal/Documents/hotsheet2');
  await page.getByRole('button',{name:'Open project',exact:true}).last().click();
  await expect(page.getByText('Use real project tickets')).toBeVisible();
  const featureRow=page.locator('[data-component="ticket-list-row"]',{hasText:'Use real project tickets'});
  await expect(featureRow.locator('[data-lucide="sparkles"]')).toBeVisible();
  await expect(featureRow.locator('.ticket-list-row__category--label')).toHaveCount(0);
  await page.getByText('Use real project tickets').click();
  await expect(page.getByText('The real ticket body.')).toBeVisible();
  await page.getByRole('button',{name:/Change status/}).click();
  await page.locator('[data-inspector-status="completed"]').click();
  await expect(page.locator('[data-component="ticket-inspector"] [data-component="status-badge"]')).toContainText('Completed');
  await page.getByRole('button',{name:'Timeline'}).click();
  await expect(page.getByText('Connected the client')).toBeVisible();
  await page.getByRole('button',{name:'Info'}).click();
  await page.getByRole('button',{name:'New ticket…'}).click();
  await page.locator('wa-input[name="new-ticket-title"]').evaluate((node:HTMLElement&{value:string})=>{node.value='Created from the real shell';node.dispatchEvent(new Event('input',{bubbles:true}))});
  await page.getByRole('button',{name:'Create ticket'}).click();
  await expect(page.getByText('Created from the real shell')).toBeVisible();
  await page.getByLabel('Settings view').click();
  await expect(page.getByText('/work/demo.hs2')).toBeVisible();
});

test('renders attachment identity from a selected real ticket',async({page})=>{
  await mockProject(page);await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();await page.getByText('Use real project tickets').click();await page.getByRole('button',{name:'Attachments'}).click();
  const item=page.locator('[data-attachment-id="A1"]');await expect(item).toContainText('proof.png');
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
  const source=page.getByRole('textbox',{name:'Ticket details'});await expect(source).toBeFocused();await source.fill('Carried into the larger editor');await page.getByRole('button',{name:'Open ticket reader'}).click();
  const reader=page.getByRole('dialog',{name:/Read and edit HS2-DEMO01/});await expect(reader).toBeVisible();await expect(reader.getByRole('textbox',{name:'Ticket details'})).toHaveValue('Carried into the larger editor');await expect(reader.getByRole('textbox',{name:'Feedback response'})).toBeVisible();await reader.getByRole('button',{name:'Close ticket reader'}).click();
  await source.fill('');await source.blur();
  await expect(page.getByText('Click to add Markdown.')).toBeVisible();
  await page.getByRole('button',{name:'Edit Ticket details'}).click();
  await expect(source).toBeFocused();await source.fill('Added from an empty ticket');await source.blur();
  await expect(page.locator('[data-component="markdown-preview"]')).toContainText('Added from an empty ticket');
});

test('keeps backlog and archived tickets out of the active Queue',async({page})=>{
  await mockProject(page);await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();
  await expect(page.getByRole('heading',{name:'Queue'})).toBeVisible();
  await expect(page.locator('[data-project-dialog]')).not.toBeVisible();
  await expect(page.getByText('Use real project tickets')).toBeVisible();
  await expect(page.getByText('Deferred backlog ticket')).toHaveCount(0);await expect(page.getByText('Archived ticket')).toHaveCount(0);
  await page.getByRole('button',{name:/Backlog/}).click();await expect(page.getByText('Deferred backlog ticket')).toBeVisible();await expect(page.getByText('Use real project tickets')).toHaveCount(0);
  await page.getByRole('button',{name:/Archive/}).click();await expect(page.getByText('Archived ticket')).toBeVisible();await expect(page.getByText('Deferred backlog ticket')).toHaveCount(0);
});

test('derives board columns from the selected view and merges Verified by project setting',async({page})=>{
  await mockProject(page);await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();
  await page.getByLabel('Columns view').click();
  const board=page.locator('.ticket-board');
  await expect(board.locator('.ticket-board-column')).toHaveCount(4);
  await expect(board.locator('.ticket-board-column__header h2')).toHaveText(['Not Started','Started','Completed','Verified']);
  await expect(board.locator('[data-column-id="completed"]')).toContainText('Completed ticket');
  await expect(board.locator('[data-column-id="verified"]')).toContainText('Verified ticket');

  await page.getByLabel('Settings view').click();
  await page.getByLabel('Hide Verified column').check();
  await expect.poll(()=>page.evaluate(()=>localStorage.getItem('hotsheet.project.demo-checkout.hide-verified-column'))).toBe('true');
  await page.getByLabel('Columns view').click();
  await expect(board.locator('.ticket-board-column__header h2')).toHaveText(['Not Started','Started','Completed']);
  await expect(board.locator('[data-column-id="completed"]')).toContainText('Completed ticket');
  await expect(board.locator('[data-column-id="completed"]')).toContainText('Verified ticket');

  await page.getByLabel('Settings view').click();
  await page.getByLabel('Hide Verified column').uncheck();
  await page.getByLabel('Columns view').click();
  await expect(board.locator('.ticket-board-column__header h2')).toHaveText(['Not Started','Started','Completed','Verified']);

  await page.getByRole('button',{name:/Backlog/}).click();
  await expect(board.locator('.ticket-board-column__header h2')).toHaveText(['Backlog']);
  await expect(board.locator('[data-column-id="backlog"]')).toContainText('Deferred backlog ticket');
  await page.getByRole('button',{name:/Archive/}).click();
  await expect(board.locator('.ticket-board-column__header h2')).toHaveText(['Archive']);
  await expect(board.locator('[data-column-id="archive"]')).toContainText('Archived ticket');
});

test('undoes, redoes, copies, pastes, and drags ticket mutations through the real shell',async({page})=>{
  await mockProject(page);await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();
  const ticket=page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-DEMO01"]');await ticket.click();
  await page.getByRole('button',{name:/Change status/}).click();await page.locator('[data-inspector-status="completed"]').click();
  await expect(page.locator('[data-component="ticket-inspector"] [data-component="status-badge"]')).toContainText('Completed');
  await page.keyboard.press('Control+z');await expect(page.locator('[data-component="ticket-inspector"] [data-component="status-badge"]')).toContainText('Started');
  await page.keyboard.press('Control+Shift+z');await expect(page.locator('[data-component="ticket-inspector"] [data-component="status-badge"]')).toContainText('Completed');
  await page.keyboard.press('Control+c');await page.keyboard.press('Control+v');await expect(page.getByText('Use real project tickets (Copy)')).toBeVisible();
  await ticket.evaluate(node=>{const transfer=new DataTransfer();node.dispatchEvent(new DragEvent('dragstart',{bubbles:true,dataTransfer:transfer}));document.querySelector<HTMLElement>('[data-ticket-drop-status="backlog"]')!.dispatchEvent(new DragEvent('dragover',{bubbles:true,cancelable:true,dataTransfer:transfer}))});
  await expect(page.locator('[data-ticket-drop-status="backlog"]')).toHaveAttribute('data-dragging-ticket','true');
  await page.locator('[data-ticket-drop-status="backlog"]').dispatchEvent('drop');
  await page.locator('[data-ticket-drop-status="backlog"]').click();await expect(page.locator('[data-component="ticket-list-row"]',{hasText:'Use real project tickets'})).toBeVisible();
});

test('preserves attachments and atomically undoes and redoes cut-paste',async({page})=>{
  const copyRequests:unknown[]=[];page.on('request',request=>{if(new URL(request.url()).pathname.endsWith('/provider-attachments/copy'))copyRequests.push(request.postDataJSON())});
  await mockProject(page);await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();
  const source=page.locator('[data-component="ticket-list-row"][data-ticket-slug="HS2-DEMO01"]');await source.click();await page.getByRole('button',{name:'Attachments'}).click();await expect(page.locator('[data-attachment-id="A1"]')).toBeVisible();await page.keyboard.press('Control+x');await page.keyboard.press('Control+v');
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
  const notificationsButton=page.getByRole('button',{name:/Notifications view/});await notificationsButton.click();await expect(notificationsButton).toHaveAttribute('aria-pressed','true');await expect(page.getByRole('button',{name:'List view'})).toHaveAttribute('aria-pressed','false');await expect(page.locator('[data-component="notification-center"]')).toBeVisible();await expect(popup).toBeVisible();await expect(page.locator('.notification-inspector-empty')).toBeVisible();await page.screenshot({path:'/private/tmp/hotsheet-permission-shell.png'});await page.setViewportSize({width:820,height:760});await expect(popup).toBeInViewport();await page.screenshot({path:'/private/tmp/hotsheet-permission-shell-narrow.png'});
  await popup.getByRole('button',{name:'Always Allow'}).click();await expect.poll(()=>answer).toEqual({decision:'allow',scope:'always'});await expect(popup).toHaveCount(0);await expect(page.getByText('allowed this kind of request')).toBeVisible();await expect(page.getByRole('button',{name:'Notifications view'})).toBeVisible();
});

test('counts permission automation only while its popup is visible',async({page})=>{
  await page.clock.install();await mockProject(page);let pending=[{id:8,connection:'codex-session',tool:'item/commandExecution/requestApproval',action:'npm test',always_allow_supported:true}],answers=0;
  await page.route('**/connections',route=>route.fulfill({json:[{id:'codex-session',tool:'Codex',project:'/work/demo',role:'worker',busy:true}]}));
  await page.route('**/permissions',route=>route.fulfill({json:pending}));
  await page.route('**/permissions/8',route=>{answers+=1;pending=[];return route.fulfill({json:{connection:'codex-session',decision:'allow',persisted:false}})});
  await page.goto('/');await page.evaluate(()=>{localStorage.setItem('hotsheet.project.demo-checkout.permission-automation',JSON.stringify({action:'allow',delayMs:15_000}))});await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();
  const popup=page.locator('[data-component="permission-request-popup"]');await expect(popup).toContainText('Automatically allowed in');await page.clock.fastForward(10_000);await expect(popup).toContainText('0:05');await popup.getByRole('button',{name:'Ignore'}).click();await expect(popup).toHaveCount(0);await page.clock.fastForward(30_000);expect(answers).toBe(0);await page.getByRole('button',{name:/Notifications view/}).click();await page.locator('[data-component="notification-center"]').getByRole('button',{name:'Allow Once'}).click();await expect.poll(()=>answers).toBe(1);
});

test('persists and restores per-project permission automation settings',async({page})=>{
  await mockProject(page);await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();await page.getByLabel('Settings view').click();
  const action=page.locator('wa-select[name="permission-automation-action"]'),delay=page.locator('wa-select[name="permission-automation-delay"]');await expect(action).toHaveJSProperty('value','off');await expect(delay).toHaveJSProperty('disabled',true);
  await action.evaluate((node:HTMLElement&{value:string})=>{node.value='deny';node.dispatchEvent(new Event('change',{bubbles:true}))});await expect(delay).toHaveJSProperty('disabled',false);await delay.evaluate((node:HTMLElement&{value:string})=>{node.value='120000';node.dispatchEvent(new Event('change',{bubbles:true}))});
  await expect.poll(()=>page.evaluate(()=>localStorage.getItem('hotsheet.project.demo-checkout.permission-automation'))).toBe('{"action":"deny","delayMs":120000}');await page.getByLabel('List view').click();await page.getByLabel('Settings view').click();await expect(action).toHaveJSProperty('value','deny');await expect(delay).toHaveJSProperty('value','120000');
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
  await page.screenshot({path:'/private/tmp/hotsheet-real-app.png',fullPage:true});
  await page.setViewportSize({width:900,height:760});
  await expect(page.locator('[data-component="ticket-list-row"]').first()).toBeVisible();
  await page.screenshot({path:'/private/tmp/hotsheet-real-app-narrow.png',fullPage:true});
});
