import {expect,test} from '@playwright/test';

test.use({deviceScaleFactor:2,video:'on'});

const capabilities={create:true,update:true,close:true,notes:true,note_edit:true,note_delete:true,attachments:true,assignment:true,review_requests:true,dependencies:true,up_next:true,close_reasons:true,claims:true,atomic_batch:true,not_working_report:true,offline_mutation:true,history:true,watch:true,provider_idempotency:true,query_fields:[]};

test('reveals a restored terminal workspace atomically and separates All Projects',async({page})=>{
  await page.setViewportSize({width:1440,height:900});
  await page.addInitScript(()=>{
    localStorage.setItem('hotsheet.open-projects',JSON.stringify(['/work/demo','/work/other']));
    localStorage.setItem('hotsheet.workspace.active-project-root.v1','/work/demo');
    localStorage.setItem('hotsheet.terminals.drawer-open','true');
    class FakeSocket extends EventTarget{
      static CONNECTING=0;static OPEN=1;static CLOSING=2;static CLOSED=3;readyState=0;binaryType='blob';
      constructor(public url:string){super();setTimeout(()=>{this.readyState=1;this.dispatchEvent(new Event('open'));this.dispatchEvent(new MessageEvent('message',{data:new TextEncoder().encode('ready % ').buffer}))})}
      send(value:unknown){if(typeof value!=='string')return;try{const resize=JSON.parse(value).resize;if(resize)this.dispatchEvent(new MessageEvent('message',{data:JSON.stringify({pty_size:{cols:resize.cols,rows:resize.rows},driven_by:resize.viewer_id})}))}catch{/* terminal input */}}
      close(){this.readyState=3;this.dispatchEvent(new CloseEvent('close'))}
    }
    Object.assign(window,{WebSocket:FakeSocket});
  });
  await page.route('**/*',async route=>{
    const request=route.request(),url=new URL(request.url()),path=url.pathname,other=path.includes('other-checkout');
    if(path==='/__hotsheet/projects/open'){const root=request.postDataJSON().root as string,id=root==='/work/other'?'other-checkout':'demo-checkout';return route.fulfill({status:201,json:{id,root,name:root.split('/').at(-1),stores:[`${root}.hs2`],apiPath:`/__hotsheet/project-api/${id}`}})}
    if(path.endsWith('/providers'))return route.fulfill({json:[{connection_id:'git-local',provider:'git',display_name:'Hot Sheet git',locator:'/tickets',default:true,capabilities}]});
    if(path.endsWith('/tickets')){await new Promise(resolve=>setTimeout(resolve,700));return route.fulfill({json:[]})}
    if(path.endsWith('/terminals')){await new Promise(resolve=>setTimeout(resolve,250));return route.fulfill({json:[{id:other?'other-shell':'demo-shell',alive:true,busy:false,cwd:other?'/work/other':'/work/demo'}]})}
    if(path.endsWith('/permissions')||path.endsWith('/connections')||path.endsWith('/commands')||path.endsWith('/command-runs')||path.endsWith('/corrupt-tickets'))return route.fulfill({json:[]});
    if(path.endsWith('/repository/status'))return route.fulfill({json:{branch:'main',ahead:0,behind:0,staged:0,unstaged:0,untracked:0,conflicted:0,clean:true}});
    if(path.endsWith('/ws/poll'))return route.fulfill({json:{cursor:Number(url.searchParams.get('since')??0),events:[],overflow:false}});
    return route.continue();
  });
  const video=page.video();await page.goto('/?dev-review=false');const restoring=page.locator('[data-component="project-restore-state"]');await expect(restoring).toBeVisible();await page.waitForTimeout(450);await expect(restoring).toBeVisible();await expect(page.locator('[data-component="app-shell"]')).toHaveCount(0);await expect(page.locator('[data-component="terminal-drawer"]')).toHaveCount(0);await page.screenshot({path:'/private/tmp/hs2-rpgs2s-atomic-restore-loading.png',fullPage:true});
  const shell=page.locator('[data-component="app-shell"]');await expect(shell).toBeVisible({timeout:5_000});await expect(restoring).toHaveCount(0);await expect(page.locator('[data-component="terminal-drawer"]')).toBeVisible();await page.getByRole('button',{name:'Terminal dashboard'}).click();const sidebar=page.locator('[data-component="terminal-operations-sidebar"]'),all=sidebar.locator('.terminal-operations-sidebar__group[data-project-id="all"]');await expect(sidebar.locator('.terminal-operations-sidebar__group')).toHaveCount(3);await expect(all).toHaveCSS('border-bottom-width','1px');await page.screenshot({path:'/private/tmp/hs2-rpgs2s-9damg3-terminal-dashboard-wide-after.png',fullPage:true});await page.setViewportSize({width:1024,height:650});await expect(all).toHaveCSS('border-bottom-width','1px');await page.screenshot({path:'/private/tmp/hs2-rpgs2s-9damg3-terminal-dashboard-narrow-after.png',fullPage:true});
  await page.close();await video?.saveAs('/private/tmp/hs2-rpgs2s-atomic-refresh-after.webm');
});
