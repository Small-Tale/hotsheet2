import { expect,test } from '@playwright/test';

const project = { id:'demo-checkout', root:'/work/demo', name:'demo', stores:['/work/demo.hs2'], apiPath:'/__hotsheet/project-api/demo-checkout' };
const row = { connection_id:'git-local', native_id:'01', qualified_id:'git-local:01', id:'01', slug:'HS2-DEMO01', title:'Use real project tickets', category:'feature', priority:'high', status:'started', up_next:true, tags:['client'], blocked_by:[], claim_count:0, created_at:'2026-08-30T00:00:00Z', updated_at:'2026-08-30T01:00:00Z' };
const backlogRow = { ...row, native_id:'03', qualified_id:'git-local:03', id:'03', slug:'HS2-BACK01', title:'Deferred backlog ticket', status:'backlog', up_next:false };
const archiveRow = { ...row, native_id:'04', qualified_id:'git-local:04', id:'04', slug:'HS2-ARCH01', title:'Archived ticket', status:'archive', up_next:false };
const notStartedRow = { ...row, native_id:'05', qualified_id:'git-local:05', id:'05', slug:'HS2-NEXT01', title:'Not started ticket', status:'not_started', up_next:false };
const completedRow = { ...row, native_id:'06', qualified_id:'git-local:06', id:'06', slug:'HS2-DONE01', title:'Completed ticket', status:'completed', up_next:false };
const verifiedRow = { ...row, native_id:'07', qualified_id:'git-local:07', id:'07', slug:'HS2-VERIFY01', title:'Verified ticket', status:'verified', up_next:false };
const full = { ...row, details:'The real ticket body.', blocked_reason:null, concurrency_token:'token', notes:[{id:'N1',kind:'activity',created_at:'2026-08-30T00:30:00Z',edited_at:'2026-08-30T00:30:00Z',text:'Connected the client\nLoaded checkout-scoped tickets.'},{id:'N2',kind:'feedback_needed',created_at:'2026-08-30T00:35:00Z',edited_at:'2026-08-30T00:35:00Z',text:'Should this reader preserve the current draft?'},{id:'N3',kind:'regular',created_at:'2026-08-30T00:36:00Z',edited_at:'2026-08-30T00:36:00Z',text:'Editable note'}], attachments:[{id:'A1',filename:'proof.png',created_at:'2026-08-30T00:40:00Z'}] };

async function mockProject(page: import('@playwright/test').Page) {
  let rows = [row,backlogRow,archiveRow,notStartedRow,completedRow,verifiedRow];
  let selectedFull = full;
  const patches: Record<string,unknown>[] = [];
  await page.route('**/*', async route => {
    const request=route.request(), url=new URL(request.url()), path=url.pathname;
    if(path==='/__hotsheet/projects/open') return route.fulfill({status:201,json:project});
    if(path.endsWith('/repository/status')) return route.fulfill({json:{branch:'main',ahead:1,behind:0,staged:0,unstaged:1,untracked:0,conflicted:0,clean:false}});
    if(path.endsWith('/tickets')&&request.method()==='GET') return route.fulfill({json:rows});
    if(path.endsWith('/tickets')&&request.method()==='POST'){const body=request.postDataJSON();const created={...row,id:'02',native_id:'02',slug:'HS2-NEW001',title:body.title,category:body.category,up_next:false};rows=[created,...rows];return route.fulfill({status:201,json:{...created,details:'',notes:[],attachments:[]}})}
    if(path.endsWith('/tickets/01')&&request.method()==='GET') return route.fulfill({json:{store:'git-local',...selectedFull}});
    if(path.endsWith('/tickets/01/attachments')&&request.method()==='POST'){const filename=request.headers()['x-hotsheet-filename']??'attachment';selectedFull={...selectedFull,attachments:[...selectedFull.attachments,{id:`A${selectedFull.attachments.length+1}`,filename,created_at:'2026-08-30T01:10:00Z'}]};return route.fulfill({status:201,json:{store:'git-local',...selectedFull}})}
    if(path.includes('/tickets/')&&request.method()==='PATCH'){const id=path.split('/').pop(),body=request.postDataJSON();patches.push(body);rows=rows.map(item=>item.id===id?{...item,...body}:item);selectedFull={...selectedFull,...body};return route.fulfill({json:{store:'git-local',...selectedFull}})}
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
});

test('edits non-empty details on double click and empty details on one click',async({page})=>{
  await mockProject(page);await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();await page.getByText('Use real project tickets').click();
  const preview=page.getByRole('button',{name:'Edit Ticket details'});
  await preview.dblclick();
  const source=page.getByRole('textbox',{name:'Ticket details'});await expect(source).toBeFocused();await source.fill('Carried into the larger editor');await page.getByRole('button',{name:'Open ticket reader'}).click();
  const reader=page.getByRole('dialog',{name:/Read and edit HS2-DEMO01/});await expect(reader).toBeVisible();await expect(reader.getByRole('textbox',{name:'Ticket details'})).toHaveValue('Carried into the larger editor');await expect(reader.getByRole('textbox',{name:'Feedback response'})).toBeVisible();await reader.getByRole('button',{name:'Cancel'}).click();await reader.getByRole('button',{name:'Close ticket reader'}).click();
  await preview.dblclick();await source.fill('');await page.getByRole('button',{name:'Save'}).click();
  await expect(page.getByText('Click to add Markdown.')).toBeVisible();
  await page.getByRole('button',{name:'Edit Ticket details'}).click();
  await expect(source).toBeFocused();await source.fill('Added from an empty ticket');await page.getByRole('button',{name:'Save'}).click();
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
  await page.getByRole('button',{name:/Backlog/}).click();await expect(page.locator('[data-component="ticket-list-row"]',{hasText:'Use real project tickets'})).toBeVisible();
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
