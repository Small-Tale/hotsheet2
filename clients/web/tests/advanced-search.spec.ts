import {expect,test} from '@playwright/test';

const base={connection_id:'git',native_id:'1',qualified_id:'git:1',id:'1',slug:'HS2-ACTIVE',title:'Active parser work',category:'feature',priority:'default',status:'started',up_next:true,feedback_needed:false,tags:['client'],blocked_by:[],claim_count:1,claimed_by:'codex',claim_lease_expires_at:'2099-01-01T00:00:00Z',created_at:'2026-09-09T00:00:00Z',updated_at:'2026-09-09T00:00:00Z'};
const rows=[base,{...base,native_id:'2',qualified_id:'git:2',id:'2',slug:'HS2-DONE',title:'Completed parser docs',status:'completed',up_next:false,claimed_by:undefined,claim_lease_expires_at:undefined},{...base,native_id:'3',qualified_id:'git:3',id:'3',slug:'HS2-VERIFIED',title:'Verified parser docs',status:'verified',up_next:false,claimed_by:undefined,claim_lease_expires_at:undefined},{...base,native_id:'4',qualified_id:'git:4',id:'4',slug:'HS2-BACKLOG',title:'Backlogged parser task',status:'backlog',up_next:false,claimed_by:undefined,claim_lease_expires_at:undefined},{...base,native_id:'5',qualified_id:'git:5',id:'5',slug:'HS2-ARCHIVE',title:'Archived parser task',status:'archive',up_next:false,claimed_by:undefined,claim_lease_expires_at:undefined}];

test('evaluates is: filters and boolean expressions in the workspace search',async({page})=>{
  await page.route('**/*',async route=>{const request=route.request(),path=new URL(request.url()).pathname;
    if(!path.startsWith('/__hotsheet/'))return route.continue();
    if(path==='/__hotsheet/projects/open')return route.fulfill({status:201,json:{id:'demo',root:'/work/demo',name:'Search demo',stores:['/work/demo.hs2'],apiPath:'/__hotsheet/project-api/demo',needsTicketSetup:false,needsHs1Migration:false,hs1ImportCompleted:false,hs1CleanupEligible:false}});
    if(path.endsWith('/tickets')&&request.method()==='GET')return route.fulfill({json:rows});
    if(path.endsWith('/providers'))return route.fulfill({json:[{connection_id:'git',provider:'git',display_name:'Hot Sheet git',locator:'/work/demo.hs2',default:true,capabilities:{create:true,update:true,notes:true,attachments:true,watch:true,query_fields:[]}}]});
    if(path.endsWith('/connections')||path.endsWith('/permissions')||path.endsWith('/commands')||path.endsWith('/terminals')||path.endsWith('/corrupt-tickets'))return route.fulfill({json:[]});
    if(path.endsWith('/repository/status'))return route.fulfill({json:{branch:'main',ahead:0,behind:0,staged:0,unstaged:0,untracked:0,conflicted:0,clean:true}});
    if(path.endsWith('/terminal-settings'))return route.fulfill({json:{inherit_global_shell_history:false}});
    if(path.endsWith('/ws/poll'))return route.fulfill({json:{cursor:0,events:[],overflow:false}});
    return route.fulfill({status:404,json:{error:'not mocked'}});
  });
  await page.setViewportSize({width:1100,height:800});await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();
  await page.getByRole('button',{name:'Search tickets'}).click();const search=page.getByRole('textbox',{name:'Search tickets'});await search.fill('is:active OR (is:completed AND NOT is:verified)');await expect(page.locator('[data-ticket-slug="HS2-ACTIVE"]')).toBeVisible();await expect(page.locator('[data-ticket-slug="HS2-DONE"]')).toBeVisible();await expect(page.locator('[data-ticket-slug="HS2-VERIFIED"]')).toHaveCount(0);await page.screenshot({path:'/private/tmp/hs2-3cnnjm-boolean-search-wide.png',fullPage:true});
  await search.fill('is:archived OR is:backlogged');await expect(page.locator('[data-ticket-slug="HS2-ARCHIVE"]')).toBeVisible();await expect(page.locator('[data-ticket-slug="HS2-BACKLOG"]')).toBeVisible();
  await page.setViewportSize({width:680,height:720});await page.getByRole('button',{name:'Search syntax help'}).click();const help=page.getByRole('dialog',{name:'Search syntax'});await expect(help).toContainText('NOT binds before AND, and AND before OR');await expect(help).toContainText('is:active');await expect.poll(()=>page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth)).toBe(true);await page.screenshot({path:'/private/tmp/hs2-3cnnjm-search-help-narrow.png',fullPage:true});
});
