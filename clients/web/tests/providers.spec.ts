import { expect,test } from '@playwright/test';

const project = { id:'demo-checkout', root:'/work/demo', name:'demo', stores:['/work/demo.hs2'], apiPath:'/__hotsheet/project-api/demo-checkout' };
const row = { connection_id:'git-local', native_id:'01', qualified_id:'git-local:01', id:'01', slug:'HS2-DEMO01', title:'Use real project tickets', category:'feature', priority:'high', status:'started', up_next:true, tags:['client'], blocked_by:[], claim_count:0, created_at:'2026-08-30T00:00:00Z', updated_at:'2026-08-30T01:00:00Z' };
const full = { ...row, details:'The real ticket body.', blocked_reason:null, concurrency_token:'token', notes:[{id:'N1',kind:'activity',created_at:'2026-08-30T00:30:00Z',edited_at:'2026-08-30T00:30:00Z',text:'Connected the client\nLoaded checkout-scoped tickets.'}], attachments:[{id:'A1',filename:'proof.png',created_at:'2026-08-30T00:40:00Z'}] };

async function mockProject(page: import('@playwright/test').Page) {
  let rows = [row];
  let selectedFull = full;
  await page.route('**/*', async route => {
    const request=route.request(), url=new URL(request.url()), path=url.pathname;
    if(path==='/__hotsheet/projects/open') return route.fulfill({status:201,json:project});
    if(path.endsWith('/repository/status')) return route.fulfill({json:{branch:'main',ahead:1,behind:0,staged:0,unstaged:1,untracked:0,conflicted:0,clean:false}});
    if(path.endsWith('/tickets')&&request.method()==='GET') return route.fulfill({json:rows});
    if(path.endsWith('/tickets')&&request.method()==='POST'){const body=request.postDataJSON();const created={...row,id:'02',native_id:'02',slug:'HS2-NEW001',title:body.title,category:body.category,up_next:false};rows=[created,...rows];return route.fulfill({status:201,json:{...created,details:'',notes:[],attachments:[]}})}
    if(path.endsWith('/tickets/01')&&request.method()==='GET') return route.fulfill({json:{store:'git-local',...selectedFull}});
    if(path.includes('/tickets/')&&request.method()==='PATCH'){const id=path.split('/').pop(),body=request.postDataJSON();rows=rows.map(item=>item.id===id?{...item,...body}:item);selectedFull={...selectedFull,...body};return route.fulfill({json:{store:'git-local',...selectedFull}})}
    return route.continue();
  });
}

test('opens a checkout, discovers its source, and drives real shell ticket flows',async({page})=>{
  await mockProject(page); await page.goto('/');
  await page.getByRole('button',{name:'Open project'}).click();
  await expect(page.locator('wa-input[name="project-root"]')).toHaveJSProperty('value','/Users/westphal/Documents/hotsheet2');
  await page.getByRole('button',{name:'Open project',exact:true}).last().click();
  await expect(page.getByText('Use real project tickets')).toBeVisible();
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
