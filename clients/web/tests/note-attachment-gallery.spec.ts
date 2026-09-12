import {expect,test} from '@playwright/test';

const row={connection_id:'git',native_id:'1',qualified_id:'git:1',id:'1',slug:'HS2-IMAGES',title:'Two note images',category:'bug',priority:'default',status:'started',up_next:true,feedback_needed:false,tags:['client'],blocked_by:[],claim_count:0,created_at:'2026-09-09T00:00:00Z',updated_at:'2026-09-09T00:00:00Z'};
const attachments=[{id:'A-FIRST',filename:'first.svg',created_at:'2026-09-09T00:01:00Z'},{id:'A-SECOND',filename:'second.svg',created_at:'2026-09-09T00:02:00Z'}];
const ticket={store:'git',...row,details:'Local evidence: `attachment:first.svg`\n\nRelated ticket: `attachment:[HS2-OTHER]report.pdf`',blocked_reason:null,concurrency_token:'token',notes:[{id:'N1',kind:'regular',created_at:'2026-09-09T00:03:00Z',edited_at:'2026-09-09T00:03:00Z',text:'First image: attachment:first.svg\n\nSecond image: attachment:second.svg'}],attachments};

test('each image in one note opens its own gallery attachment',async({page})=>{
  await page.route('**/*',async route=>{const request=route.request(),url=new URL(request.url()),path=url.pathname;
    if(!path.startsWith('/__hotsheet/'))return route.continue();
    if(path==='/__hotsheet/projects/open')return route.fulfill({status:201,json:{id:'demo',root:'/work/demo',name:'Image demo',stores:['/work/demo.hs2'],apiPath:'/__hotsheet/project-api/demo',needsTicketSetup:false,needsHs1Migration:false,hs1ImportCompleted:false,hs1CleanupEligible:false}});
    if(path.endsWith('/tickets')&&request.method()==='GET')return route.fulfill({json:url.searchParams.has('page_size')?{items:[row],counts:{all:1,queue:1,backlog:0,archive:0}}:[row]});
    if(path.endsWith('/tickets/1')&&request.method()==='GET')return route.fulfill({json:ticket});
    if(path.endsWith('/providers'))return route.fulfill({json:[{connection_id:'git',provider:'git',display_name:'Hot Sheet git',locator:'/work/demo.hs2',default:true,capabilities:{create:true,update:true,notes:true,attachments:true,watch:true,query_fields:[]}}]});
    if(path.endsWith('/connections')||path.endsWith('/permissions')||path.endsWith('/commands')||path.endsWith('/terminals')||path.endsWith('/corrupt-tickets'))return route.fulfill({json:[]});
    if(path.endsWith('/repository/status'))return route.fulfill({json:{branch:'main',ahead:0,behind:0,staged:0,unstaged:0,untracked:0,conflicted:0,clean:true}});
    if(path.endsWith('/terminal-settings'))return route.fulfill({json:{inherit_global_shell_history:false}});
    if(path.endsWith('/ws/poll'))return route.fulfill({json:{cursor:0,events:[],overflow:false}});
    const media=path.match(/\/attachments\/(A-FIRST|A-SECOND)$/)?.[1];
    if(media){const label=media==='A-FIRST'?'FIRST':'SECOND',color=media==='A-FIRST'?'#2d73da':'#8754c8';return route.fulfill({contentType:'image/svg+xml',body:`<svg xmlns="http://www.w3.org/2000/svg" width="640" height="360"><rect width="640" height="360" fill="${color}"/><text x="320" y="190" fill="white" font-size="64" text-anchor="middle">${label}</text></svg>`})}
    return route.fulfill({status:404,json:{error:'not mocked'}});
  });
  await page.setViewportSize({width:1280,height:720});await page.goto('/?dev-review=false');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();await page.locator('[data-ticket-slug="HS2-IMAGES"]').click();
  const details=page.locator('.ticket-inspector__details-surface');await expect(details.getByRole('button',{name:'Open first.svg in image gallery'})).toHaveAttribute('data-attachment-url',/\/tickets\/HS2-IMAGES\/attachments\/A-FIRST$/);await expect(details.getByRole('link',{name:'report.pdf'})).toHaveAttribute('href',/\/tickets\/HS2-OTHER\/attachments\/by-name\/report.pdf$/);await details.screenshot({path:'/private/tmp/hs2-w2mwf3-details-attachments-wide.png'});await page.setViewportSize({width:940,height:720});await details.screenshot({path:'/private/tmp/hs2-w2mwf3-details-attachments-narrow.png'});
  const note=page.locator('article[data-note-id="N1"]'),first=note.getByRole('button',{name:'Open first.svg in image gallery'}),second=note.getByRole('button',{name:'Open second.svg in image gallery'});await expect(first).toHaveAttribute('data-gallery-attachment-id','A-FIRST');await expect(second).toHaveAttribute('data-gallery-attachment-id','A-SECOND');
  await second.click();let gallery=page.getByRole('dialog',{name:'Image 2 of 2: second.svg'});await expect(gallery).toBeVisible();await expect(gallery.locator('img')).toHaveAttribute('src',/\/attachments\/A-SECOND$/);await page.screenshot({path:'/private/tmp/hs2-dafv82-note-second-image-wide.png',fullPage:true});await page.keyboard.press('Escape');
  await first.click();gallery=page.getByRole('dialog',{name:'Image 1 of 2: first.svg'});await expect(gallery).toBeVisible();await expect(gallery.locator('img')).toHaveAttribute('src',/\/attachments\/A-FIRST$/);await page.setViewportSize({width:940,height:720});await page.screenshot({path:'/private/tmp/hs2-dafv82-note-first-image-narrow.png',fullPage:true});
});
