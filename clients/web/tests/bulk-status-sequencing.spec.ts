import { expect, type Page,test } from '@playwright/test';

const project = {id:'demo-checkout',root:'/work/demo',name:'Bulk status demo',stores:['/work/demo.hs2'],apiPath:'/__hotsheet/project-api/demo-checkout',needsTicketSetup:false,needsHs1Migration:false,hs1ImportCompleted:false,hs1CleanupEligible:false};
const base = {connection_id:'git-local',category:'issue',priority:'default',status:'completed',up_next:false,feedback_needed:false,tags:['client'],blocked_by:[],claim_count:0,created_at:'2026-09-11T01:00:00Z',updated_at:'2026-09-11T01:00:00Z'};

async function mockBulkProject(page:Page,{conflictFirst=false}:{conflictFirst?:boolean}={}) {
  let rows = [
    {...base,native_id:'01',qualified_id:'git-local:01',id:'01',slug:'HS2-FAST01',title:'First fast bulk ticket'},
    {...base,native_id:'02',qualified_id:'git-local:02',id:'02',slug:'HS2-FAST02',title:'Second fast bulk ticket'},
  ];
  const batches:Array<Array<{id:string;status:string;expected_token?:string}>>=[];
  let releaseVerified!:()=>void;
  const verifiedResponse=new Promise<void>(resolve=>{releaseVerified=resolve});
  await page.addInitScript(()=>{
    if(!new URLSearchParams(location.search).has('dev-review')){
      const url=new URL(location.href);url.searchParams.set('dev-review','false');history.replaceState(null,'',url);
    }
  });
  await page.route('**/*',async route=>{
    const request=route.request(),path=new URL(request.url()).pathname;
    if(!path.startsWith('/__hotsheet/'))return route.continue();
    if(path==='/__hotsheet/projects/open')return route.fulfill({status:201,json:project});
    if(path.endsWith('/providers'))return route.fulfill({json:[{connection_id:'git-local',provider:'git',display_name:'Hot Sheet git',locator:'/work/demo.hs2',default:true,capabilities:{create:true,update:true,close:true,notes:true,attachments:true,up_next:true,atomic_batch:true,watch:true,query_fields:[]}}]});
    if(path.endsWith('/tickets')&&request.method()==='GET')return route.fulfill({json:rows});
    if(path.endsWith('/batch')&&request.method()==='POST'){
      const updates=request.postDataJSON().updates as Array<{id:string;status:string;expected_token?:string}>;
      batches.push(updates);
      if(conflictFirst&&batches.length===1)return route.fulfill({status:409,json:{error:'Ticket modified concurrently by another client.'}});
      const stale=updates.find(update=>rows.find(row=>row.id===update.id)?.updated_at!==update.expected_token);
      if(stale)return route.fulfill({status:409,json:{error:`Ticket ${stale.id} modified concurrently.`}});
      const token=`2026-09-11T01:0${batches.length}:00Z`;
      rows=rows.map(row=>{const update=updates.find(item=>item.id===row.id);return update?{...row,status:update.status,updated_at:token}:row});
      const changed=updates.map(update=>({...rows.find(row=>row.id===update.id)!,store:'git-local',details:'',blocked_reason:null,notes:[],attachments:[],concurrency_token:token}));
      if(updates.every(update=>update.status==='verified'))await verifiedResponse;
      return route.fulfill({json:changed});
    }
    if(path.includes('/tickets/')&&request.method()==='GET'){
      const id=path.split('/').pop(),ticket=rows.find(row=>row.id===id);
      return route.fulfill({json:{store:'git-local',...ticket,details:'',blocked_reason:null,notes:[],attachments:[],concurrency_token:ticket?.updated_at}});
    }
    if(path.endsWith('/permissions')||path.endsWith('/connections')||path.endsWith('/commands')||path.endsWith('/terminals')||path.endsWith('/corrupt-tickets'))return route.fulfill({json:[]});
    if(path.endsWith('/repository/status'))return route.fulfill({json:{branch:'main',ahead:0,behind:0,staged:0,unstaged:0,untracked:0,conflicted:0,clean:true}});
    if(path.endsWith('/terminal-settings'))return route.fulfill({json:{inherit_global_shell_history:false}});
    if(path.endsWith('/ws/poll'))return route.fulfill({json:{cursor:0,events:[],overflow:false}});
    return route.fulfill({status:404,json:{error:'not mocked'}});
  });
  return {batches,releaseVerified,getRows:()=>rows};
}

async function openBulkProject(page:Page) {
  await page.goto('/');
  await page.getByRole('button',{name:'Open project'}).click();
  await page.getByRole('button',{name:'Open project',exact:true}).last().click();
  await page.getByRole('button',{name:'Columns view'}).click();
}

test('serializes rapid Verified then Archive batches onto fresh concurrency tokens (HS2-0K2ZP3)',async({page})=>{
  const mock=await mockBulkProject(page);
  await page.setViewportSize({width:1600,height:900});
  await openBulkProject(page);
  const menu=page.getByRole('menu',{name:'Ticket actions'}),completed=page.locator('[data-column-id="completed"]'),first=completed.locator('[data-ticket-slug="HS2-FAST01"]'),second=completed.locator('[data-ticket-slug="HS2-FAST02"]');
  await first.click();await second.click({modifiers:['Meta']});await first.click({button:'right'});await menu.locator('[data-context-action="Verify ticket"]').click();
  await expect.poll(()=>mock.batches.length).toBe(1);
  const verified=page.locator('[data-column-id="verified"]'),verifiedFirst=verified.locator('[data-ticket-slug="HS2-FAST01"]');
  await expect(verified.locator('[data-selected="true"]')).toHaveCount(2);
  await verifiedFirst.click({button:'right'});await menu.locator('[data-context-action="Archive ticket"]').click();
  await page.waitForTimeout(150);
  expect(mock.batches).toHaveLength(1);
  expect(mock.batches[0].map(update=>update.expected_token)).toEqual(['2026-09-11T01:00:00Z','2026-09-11T01:00:00Z']);
  mock.releaseVerified();
  await expect.poll(()=>mock.batches.length).toBe(2);
  expect(mock.batches[1].map(update=>update.expected_token)).toEqual(['2026-09-11T01:01:00Z','2026-09-11T01:01:00Z']);
  await expect.poll(()=>mock.getRows().map(row=>row.status)).toEqual(['archive','archive']);
  await page.locator('[data-action="select-view"][data-item-id="archive"]').click();
  await expect(page.locator('[data-ticket-slug="HS2-FAST01"]')).toBeVisible();
  await expect(page.locator('[data-ticket-slug="HS2-FAST02"]')).toBeVisible();
  await expect(page.getByRole('alert')).toHaveCount(0);
  await page.screenshot({path:'/private/tmp/hs2-0k2zp3-sequenced-bulk-wide.png',fullPage:true});
  await page.setViewportSize({width:1024,height:720});
  await page.screenshot({path:'/private/tmp/hs2-0k2zp3-sequenced-bulk-narrow.png',fullPage:true});
});

test('rolls a genuine external atomic conflict back to the prior status',async({page})=>{
  await mockBulkProject(page,{conflictFirst:true});
  await openBulkProject(page);
  const first=page.locator('[data-column-id="completed"] [data-ticket-slug="HS2-FAST01"]'),second=page.locator('[data-column-id="completed"] [data-ticket-slug="HS2-FAST02"]'),menu=page.getByRole('menu',{name:'Ticket actions'});
  await first.click();await second.click({modifiers:['Meta']});await first.click({button:'right'});await menu.locator('[data-context-action="Verify ticket"]').click();
  await expect(page.getByRole('alert')).toContainText('modified concurrently');
  await expect(page.locator('[data-column-id="completed"] [data-ticket-slug="HS2-FAST01"]')).toBeVisible();
  await expect(page.locator('[data-column-id="completed"] [data-ticket-slug="HS2-FAST02"]')).toBeVisible();
  await expect(page.locator('[data-column-id="verified"] [data-ticket-slug^="HS2-FAST"]')).toHaveCount(0);
});
