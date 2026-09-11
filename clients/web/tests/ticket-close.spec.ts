import { expect, test } from '@playwright/test';

const sourceProject={id:'source-project',root:'/work/source',name:'Kerf',stores:['/work/source.hs2'],apiPath:'/__hotsheet/project-api/source-project'};
const targetProject={id:'target-project',root:'/work/target',name:'Hot Sheet 2',stores:['/work/target.hs2'],apiPath:'/__hotsheet/project-api/target-project'};
const source={connection_id:'git-source',native_id:'source',qualified_id:'git-source:source',id:'source',slug:'KF-1AAYP9',title:'Repeated stability report',category:'bug',priority:'default',status:'started',up_next:false,feedback_needed:false,tags:['client'],blocked_by:[],claim_count:0,created_at:'2026-09-09T00:00:00Z',updated_at:'2026-09-09T01:00:00Z'};
const canonical={...source,connection_id:'git-target',native_id:'canonical',qualified_id:'git-target:canonical',id:'canonical',slug:'HS2-1S6DS9',title:'Canonical checkout registry repair',status:'completed'};
const collision={...source,native_id:'collision',qualified_id:'git-source:collision',id:'collision',slug:'HS2-1S6DS9',title:'Different local ticket with the same slug'};
const full=(ticket:typeof source,extra:Record<string,unknown>={})=>({...ticket,details:'Diagnostic report.',blocked_reason:null,concurrency_token:`token-${ticket.id}`,notes:[],attachments:[],...extra});
const capabilities={create:true,update:true,close:true,notes:true,note_edit:true,note_delete:true,attachments:true,assignment:true,review_requests:true,dependencies:true,up_next:true,close_reasons:true,claims:true,atomic_batch:true,not_working_report:true,offline_mutation:true,history:true,watch:true,provider_idempotency:true,query_fields:[]};

test('marks a ticket as a duplicate of an exact ticket in another project and reopens it',async({page})=>{
  let sourceRows=[source,collision],closeBody:Record<string,unknown>|undefined;
  await page.route('**/*',async route=>{
    const request=route.request(),url=new URL(request.url()),path=decodeURIComponent(url.pathname);
    if(path==='/__hotsheet/projects/open'){
      const root=(request.postDataJSON() as {root:string}).root;
      return route.fulfill({status:201,json:root==='/work/target'?targetProject:sourceProject});
    }
    if(path==='/__hotsheet/folders/choose')return route.fulfill({json:{path:'/work/target'}});
    const target=path.includes('/target-project/'),rows=target?[canonical]:sourceRows,provider=target?'git-target':'git-source';
    if(path.endsWith('/providers'))return route.fulfill({json:[{connection_id:provider,provider:'git',display_name:target?'Hot Sheet git':'Kerf git',locator:target?'/work/target.hs2':'/work/source.hs2',default:true,capabilities}]});
    if(path.endsWith('/provider-connections'))return route.fulfill({json:[]});
    if(path.endsWith('/corrupt-tickets'))return route.fulfill({json:[]});
    if(path.endsWith('/repository/status'))return route.fulfill({json:{branch:'main',ahead:0,behind:0,staged:0,unstaged:0,untracked:0,conflicted:0,clean:true}});
    if(path.endsWith('/commands')||path.endsWith('/terminals')||path.endsWith('/permissions')||path.endsWith('/connections')||path.endsWith('/drive/sessions'))return route.fulfill({json:[]});
    if(path.endsWith('/terminal-settings'))return route.fulfill({json:{inherit_global_shell_history:false}});
    if(path.endsWith('/ws/poll')){await new Promise(resolve=>setTimeout(resolve,500));return route.fulfill({json:{cursor:0,events:[],overflow:false}})}
    if(path.endsWith('/duplicate-backlinks'))return route.fulfill({json:target?{backlinks:[{reference:'@source-project/git-source:source',project_id:'source-project',project_name:'Kerf',connection_id:'git-source',native_id:'source',qualified_id:'git-source:source',slug:'KF-1AAYP9',title:'Repeated stability report'},{reference:'@mirror-project/git-mirror:source',project_id:'mirror-project',project_name:'Kerf mirror',connection_id:'git-mirror',native_id:'source',qualified_id:'git-mirror:source',slug:'KF-1AAYP9',title:'Repeated stability report mirror'}],inaccessible_projects:[{project_id:'offline-project',project_name:'Offline archive'}]}:{backlinks:[],inaccessible_projects:[]}});
    if(path.endsWith('/tickets/git-source:source/close')&&request.method()==='POST'){
      closeBody=request.postDataJSON();sourceRows=sourceRows.map(item=>({...item,status:'completed'}));
      return route.fulfill({json:{store:'git-source',...full({...source,status:'completed'},{closed_at:'2026-09-09T02:00:00Z',close_reason:'duplicate',duplicate_of:'@target-project/git-target:canonical'})}});
    }
    if((path.endsWith('/tickets/git-source:source')||path.endsWith('/tickets/source'))&&request.method()==='GET')return route.fulfill({json:{store:'git-source',...full(sourceRows[0])}});
    if((path.endsWith('/tickets/git-target:canonical')||path.endsWith('/tickets/canonical'))&&request.method()==='GET')return route.fulfill({json:{store:'git-target',...full(canonical)}});
    if(path.endsWith('/tickets')&&request.method()==='GET'){
      const query=url.searchParams.get('text');
      return route.fulfill({json:query?(target?[canonical]:[collision]):rows});
    }
    if(!path.startsWith('/__hotsheet/'))return route.continue();
    return route.fulfill({status:404,json:{error:`Unhandled ${request.method()} ${path}`}});
  });

  await page.setViewportSize({width:1080,height:1190});await page.goto('/?dev-review=false');
  await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();
  await page.getByRole('button',{name:'Add project'}).click();await expect(page.getByRole('tab',{name:'Hot Sheet 2'})).toHaveAttribute('aria-selected','true');await page.getByRole('tab',{name:'Kerf'}).click();
  await page.locator('[data-ticket-slug="KF-1AAYP9"]').click({button:'right'});await page.getByRole('menu',{name:'Ticket actions'}).getByText('Close ticket…').click();
  const dialog=page.locator('[data-component="ticket-close-dialog"]').last();await expect(dialog).toHaveJSProperty('open',true);const reason=dialog.locator('wa-select[name="ticket-close-reason"]');await reason.click();await reason.locator('wa-option[value="duplicate"]').click();
  const search=dialog.getByRole('textbox',{name:'Existing ticket'});await search.fill('HS2-1S6DS9');const candidate=dialog.getByRole('button',{name:/HS2-1S6DS9.*Hot Sheet 2.*Canonical checkout registry repair/});await expect(dialog.getByRole('button',{name:/HS2-1S6DS9.*Kerf.*Different local ticket/})).toBeVisible();await expect(candidate).toBeVisible();await page.screenshot({path:'/private/tmp/hs2-sbw2xd-cross-project-duplicate-wide.png',fullPage:true});
  await page.setViewportSize({width:640,height:760});await expect(candidate).toBeVisible();await page.screenshot({path:'/private/tmp/hs2-sbw2xd-cross-project-duplicate-narrow.png',fullPage:true});
  await candidate.click();await expect(dialog.getByText('Hot Sheet 2',{exact:true})).toBeVisible();await dialog.getByRole('button',{name:'Mark as duplicate'}).click();
  await expect.poll(()=>closeBody).toEqual({reason:'duplicate',duplicate_of:{project_id:'target-project',connection_id:'git-target',native_id:'canonical'}});
  const inspector=page.locator('[data-component="ticket-inspector"]');await expect(inspector.locator('[data-close-reason="duplicate"]')).toContainText('Duplicate of HS2-1S6DS9 · Hot Sheet 2');await page.getByRole('tab',{name:'Hot Sheet 2'}).click();await page.locator('[data-ticket-slug="HS2-1S6DS9"]').click();await expect(inspector).toHaveAttribute('data-ticket-slug','HS2-1S6DS9');
  const backlinks=inspector.locator('[data-component="ticket-duplicate-backlinks"]');await expect(backlinks).toContainText('Duplicates 2');await expect(backlinks.getByRole('button',{name:'Open duplicate KF-1AAYP9 from Kerf',exact:true})).toBeVisible();await expect(backlinks.getByRole('button',{name:'Open duplicate KF-1AAYP9 from Kerf mirror',exact:true})).toBeVisible();await expect(backlinks).toContainText('Could not check Offline archive for additional duplicates.');await page.waitForTimeout(800);await page.setViewportSize({width:1080,height:900});await page.screenshot({path:'/private/tmp/hs2-heqr6e-duplicate-backlinks-wide.png',fullPage:true});await page.setViewportSize({width:1024,height:700});await expect(backlinks).toBeVisible();await page.screenshot({path:'/private/tmp/hs2-heqr6e-duplicate-backlinks-narrow.png',fullPage:true});
});
