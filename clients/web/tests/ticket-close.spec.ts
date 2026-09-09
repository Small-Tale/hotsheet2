import { expect, test } from '@playwright/test';

const project={id:'close-demo',root:'/work/close-demo',name:'close-demo',stores:['/work/close-demo.hs2'],apiPath:'/__hotsheet/project-api/close-demo'};
const source={connection_id:'git-local',native_id:'source',qualified_id:'git-local:source',id:'source',slug:'HS2-SOURCE',title:'Repeated stability report',category:'bug',priority:'default',status:'started',up_next:false,feedback_needed:false,tags:['client'],blocked_by:[],claim_count:0,created_at:'2026-09-09T00:00:00Z',updated_at:'2026-09-09T01:00:00Z'};
const canonical={...source,native_id:'canonical',qualified_id:'git-local:canonical',id:'canonical',slug:'HS2-CANON',title:'Canonical UI stability investigation',status:'completed'};
const full=(ticket:typeof source,extra:Record<string,unknown>={})=>({...ticket,details:'Diagnostic report.',blocked_reason:null,concurrency_token:`token-${ticket.id}`,notes:[],attachments:[],...extra});

test('marks a ticket as a structured duplicate and opens its canonical target',async({page})=>{
  let rows=[source,canonical],closeBody:Record<string,unknown>|undefined;
  await page.route('**/*',async route=>{
    const request=route.request(),url=new URL(request.url()),path=url.pathname;
    if(path==='/__hotsheet/projects/open')return route.fulfill({status:201,json:project});
    if(path.endsWith('/providers'))return route.fulfill({json:[{connection_id:'git-local',provider:'git',display_name:'Hot Sheet git',locator:'/work/close-demo.hs2',default:true,capabilities:{create:true,update:true,close:true,notes:true,note_edit:true,note_delete:true,attachments:true,assignment:true,review_requests:true,dependencies:true,up_next:true,close_reasons:true,claims:true,atomic_batch:true,not_working_report:true,offline_mutation:true,history:true,watch:true,provider_idempotency:true,query_fields:[]}}]});
    if(path.endsWith('/provider-connections'))return route.fulfill({json:[]});
    if(path.endsWith('/corrupt-tickets'))return route.fulfill({json:[]});
    if(path.endsWith('/repository/status'))return route.fulfill({json:{branch:'main',ahead:0,behind:0,staged:0,unstaged:0,untracked:0,conflicted:0,clean:true}});
    if(path.endsWith('/commands')||path.endsWith('/terminals')||path.endsWith('/permissions')||path.endsWith('/connections')||path.endsWith('/drive/sessions'))return route.fulfill({json:[]});
    if(path.endsWith('/terminal-settings'))return route.fulfill({json:{inherit_global_shell_history:false}});
    if(path.endsWith('/ws/poll')){await new Promise(resolve=>setTimeout(resolve,500));return route.fulfill({json:{cursor:0,events:[],overflow:false}})}
    const close=path.match(/\/tickets\/source\/close$/);if(close&&request.method()==='POST'){closeBody=request.postDataJSON();rows=rows.map(item=>item.id==='source'?{...item,status:'completed'}:item);return route.fulfill({json:{store:'git-local',...full({...source,status:'completed'},{closed_at:'2026-09-09T02:00:00Z',close_reason:'duplicate',duplicate_of:'canonical'})}})}
    const ticket=path.match(/\/tickets\/(source|canonical)$/);if(ticket&&request.method()==='GET'){const item=ticket[1]==='source'?source:canonical;return route.fulfill({json:{store:'git-local',...full(item)}})}
    if(path.endsWith('/tickets')&&request.method()==='GET'){const query=url.searchParams.get('text');return route.fulfill({json:query?[canonical]:rows})}
    if(!path.startsWith('/__hotsheet/'))return route.continue();
    return route.fulfill({status:404,json:{error:`Unhandled ${request.method()} ${path}`}});
  });
  await page.setViewportSize({width:1280,height:840});await page.goto('/?dev-review=false');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();
  await page.locator('[data-ticket-slug="HS2-SOURCE"]').click({button:'right'});await page.getByRole('menu',{name:'Ticket actions'}).getByText('Close ticket…').click();
  const dialog=page.locator('[data-component="ticket-close-dialog"]');await expect(dialog).toHaveJSProperty('open',true);const reason=dialog.locator('wa-select[name="ticket-close-reason"]');await reason.click();await reason.locator('wa-option[value="duplicate"]').click();
  const search=dialog.getByRole('textbox',{name:'Existing ticket'});await search.fill('Canonical');await expect(dialog.getByRole('button',{name:/HS2-CANON/})).toBeVisible();await page.screenshot({path:'/private/tmp/hs2-ftbfs5-duplicate-search-wide.png',fullPage:true});await dialog.getByRole('button',{name:/HS2-CANON/}).click();await expect(dialog.getByText('Canonical UI stability investigation')).toBeVisible();await dialog.getByRole('button',{name:'Mark as duplicate'}).click();
  await expect.poll(()=>closeBody).toEqual({reason:'duplicate',duplicate_of:'canonical'});const inspector=page.locator('[data-component="ticket-inspector"]');await expect(inspector.locator('[data-close-reason="duplicate"]')).toContainText('Duplicate of HS2-CANON');await page.setViewportSize({width:1024,height:650});await page.screenshot({path:'/private/tmp/hs2-ftbfs5-duplicate-outcome-narrow.png',fullPage:true});await inspector.getByRole('button',{name:'HS2-CANON'}).click();await expect(inspector).toHaveAttribute('data-ticket-slug','HS2-CANON');
});
