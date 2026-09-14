import {expect,test} from '@playwright/test';

const capabilities={create:true,update:true,close:true,notes:true,note_edit:true,note_delete:true,attachments:true,assignment:true,review_requests:true,dependencies:true,up_next:true,close_reasons:true,claims:true,atomic_batch:true,not_working_report:true,offline_mutation:true,history:true,watch:true,provider_idempotency:true,query_fields:[]};
const project=(id:string,root:string)=>({id,root,name:root.split('/').at(-1),stores:[`${root}.hs2`],apiPath:`/__hotsheet/project-api/${id}`});
const ticket=(slug:string,status:string)=>({connection_id:'git-local',native_id:slug,qualified_id:`git-local:${slug}`,id:slug,slug,title:slug,status,up_next:false,feedback_needed:false,tags:[],blocked_by:[],claim_count:0});

test('never renders one project sidebar with another project statistics',async({page})=>{
  let releaseOther!:()=>void;const otherGate=new Promise<void>(resolve=>{releaseOther=resolve});
  await page.setViewportSize({width:1280,height:800});
  await page.route('**/*',async route=>{
    const request=route.request(),url=new URL(request.url()),path=url.pathname,other=path.includes('other-checkout');
    if(path==='/__hotsheet/projects/open'){const root=request.postDataJSON().root as string;return route.fulfill({status:201,json:root==='/work/other'?project('other-checkout',root):project('demo-checkout',root)})}
    if(path==='/__hotsheet/folders/choose')return route.fulfill({json:{path:'/work/other'}});
    if(path.endsWith('/providers'))return route.fulfill({json:[{connection_id:'git-local',provider:'git',display_name:'Hot Sheet git',locator:'/tickets',default:true,capabilities}]});
    if(/\/tickets\/HS2-DEMO1$/.test(path))return route.fulfill({json:{ticket:{...ticket('HS2-DEMO1','started'),category:'bug',priority:'default',details:'',notes:[],attachments:[],created_at:'2026-09-01T00:00:00Z',updated_at:'2026-09-14T00:00:00Z'}}});
    if(path.endsWith('/tickets')){if(other)await otherGate;const counts=other?{total:2,queued:2,backlog:0,archive:0,open:2,up_next:0,active:0,started:1,completed_today:1,completion_trend:[0,0,0,0,1,0,1]}:{total:5,queued:5,backlog:0,archive:0,open:5,up_next:0,active:0,started:4,completed_today:2,completion_trend:[0,1,0,2,0,1,2]};return route.fulfill({json:{items:other?[ticket('HS2-OTHER1','started'),ticket('HS2-OTHER2','not_started')]:[ticket('HS2-DEMO1','started')],counts}})}
    if(path.endsWith('/repository/status'))return route.fulfill({json:{branch:'main',ahead:0,behind:0,staged:0,unstaged:0,untracked:0,conflicted:0,clean:true}});
    if(path.endsWith('/ws/poll'))return route.fulfill({json:{cursor:Number(url.searchParams.get('since')??0),events:[],overflow:false}});
    if(path.endsWith('/terminals')||path.endsWith('/connections')||path.endsWith('/commands')||path.endsWith('/command-runs')||path.endsWith('/views')||path.endsWith('/corrupt-tickets'))return route.fulfill({json:[]});
    return route.continue();
  });
  await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();
  const summary=page.locator('[data-component="project-sidebar"] [data-component="project-summary"]'),chart=summary.getByRole('img');
  await expect(summary).toHaveAccessibleName('Open project statistics: 2 completed today, 4 in progress');
  await expect(chart).toHaveAttribute('aria-label','Tickets completed over the last 7 days: 0, 1, 0, 2, 0, 1, 2');
  await page.getByRole('button',{name:'Add project'}).click();await expect(page.getByRole('tab',{name:'other'})).toHaveAttribute('aria-selected','true');
  await expect(summary).toHaveAccessibleName('Open project statistics: 0 completed today, 0 in progress');
  await expect(chart).toHaveAttribute('aria-label','Tickets completed over the last 7 days: 0, 0, 0, 0, 0, 0, 0');
  releaseOther();await expect(summary).toHaveAccessibleName('Open project statistics: 1 completed today, 1 in progress');
  await expect(chart).toHaveAttribute('aria-label','Tickets completed over the last 7 days: 0, 0, 0, 0, 1, 0, 1');
  await page.getByRole('tab',{name:'hotsheet2'}).click();await expect(summary).toHaveAccessibleName('Open project statistics: 2 completed today, 4 in progress');
  await expect(chart).toHaveAttribute('aria-label','Tickets completed over the last 7 days: 0, 1, 0, 2, 0, 1, 2');
  await expect(page.locator('[data-ticket-slug="HS2-DEMO1"]')).toBeVisible();await expect(page.locator('[data-ticket-slug^="HS2-OTHER"]')).toHaveCount(0);
  await page.locator('[data-ticket-slug="HS2-DEMO1"]').click();await expect(page.locator('[data-component="ticket-reader"][aria-hidden="true"]')).toBeHidden();await expect(summary).toHaveAccessibleName('Open project statistics: 2 completed today, 4 in progress');
  await page.getByRole('button',{name:'New ticket…'}).click();await page.getByRole('dialog',{name:'Create ticket'}).getByRole('button',{name:'Cancel'}).click();await expect(page.locator('[data-component="quick-ticket-composer"]')).toBeHidden();await expect(summary).toHaveAccessibleName('Open project statistics: 2 completed today, 4 in progress');await page.waitForTimeout(350);
  await page.screenshot({path:'/private/tmp/hs2-brdmbb-project-stats-wide.png',fullPage:true});await page.setViewportSize({width:1024,height:600});await page.waitForTimeout(300);await page.screenshot({path:'/private/tmp/hs2-brdmbb-project-stats-narrow.png',fullPage:true});
});
