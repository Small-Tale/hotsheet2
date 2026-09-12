import {expect,test} from '@playwright/test';

const project={id:'drawer-order',root:'/work/drawer-order',name:'Drawer order',stores:['/work/drawer-order.hs2'],apiPath:'/__hotsheet/project-api/drawer-order'};
const capabilities={create:true,update:true,close:true,notes:true,note_edit:true,note_delete:true,attachments:true,assignment:true,review_requests:true,dependencies:true,up_next:true,close_reasons:true,claims:true,atomic_batch:true,not_working_report:true,offline_mutation:true,history:true,watch:true,provider_idempotency:true,query_fields:[]};

async function installFixture(page:import('@playwright/test').Page){
  await page.addInitScript(()=>{
    class FakeSocket extends EventTarget{
      static CONNECTING=0;static OPEN=1;static CLOSING=2;static CLOSED=3;readyState=0;binaryType='blob';
      constructor(public url:string){super();setTimeout(()=>{this.readyState=1;this.dispatchEvent(new Event('open'));this.dispatchEvent(new MessageEvent('message',{data:new TextEncoder().encode('drawer order ready').buffer}))})}
      send(value:unknown){if(typeof value!=='string')return;try{const resize=JSON.parse(value).resize;if(resize)this.dispatchEvent(new MessageEvent('message',{data:JSON.stringify({pty_size:{cols:resize.cols,rows:resize.rows},driven_by:resize.viewer_id})}))}catch{/* terminal input */}}
      close(){this.readyState=3;this.dispatchEvent(new CloseEvent('close'))}
    }
    Object.assign(window,{WebSocket:FakeSocket});
  });
  let terminals=[{id:'shell-one',alive:true,busy:false,cwd:project.root},{id:'shell-two',alive:true,busy:false,cwd:project.root}];
  await page.route('**/*',route=>{
    const request=route.request(),url=new URL(request.url()),path=url.pathname;
    if(path==='/__hotsheet/projects/open')return route.fulfill({status:201,json:project});
    if(path.endsWith('/providers'))return route.fulfill({json:[{connection_id:'git-local',provider:'git',display_name:'Hot Sheet git',locator:'/tickets',default:true,capabilities}]});
    if(path.endsWith('/ai-tools'))return route.fulfill({json:[{id:'codex',display_name:'Codex',models:[{id:'gpt-6-astra',label:'GPT-6 Astra',effort_levels:['medium']}],default_model:'gpt-6-astra',default_effort:'medium',actions:['change_model','change_effort']}]});
    if(path.endsWith('/ai-settings'))return route.fulfill({json:{tool:'codex',model:'gpt-6-astra',effort:'medium'}});
    if(path.endsWith('/drive/connections')&&request.method()==='POST'){const body=request.postDataJSON();return route.fulfill({status:201,json:{id:body.connection_id,tool:body.tool,project:project.stores[0],role:'main',busy:false,actions:['send_turn','interrupt'],model:body.model,effort:body.effort}})}
    if(path.endsWith('/connections')||path.endsWith('/permissions')||path.endsWith('/commands')||path.endsWith('/command-runs')||path.endsWith('/tickets')||path.endsWith('/corrupt-tickets'))return route.fulfill({json:[]});
    if(path.endsWith('/terminals')&&request.method()==='GET')return route.fulfill({json:terminals});
    const terminal=path.match(/\/terminals\/([^/]+)$/);if(terminal&&request.method()==='DELETE'){terminals=terminals.filter(item=>item.id!==decodeURIComponent(terminal[1]));return route.fulfill({status:204})}
    if(path.endsWith('/repository/status'))return route.fulfill({json:{branch:'main',ahead:0,behind:0,staged:0,unstaged:0,untracked:0,conflicted:0,clean:true}});
    if(path.endsWith('/ws/poll'))return route.fulfill({json:{cursor:Number(url.searchParams.get('since')??0),events:[],overflow:false}});
    return route.continue();
  });
}

async function drawerOrder(drawer:import('@playwright/test').Locator){return drawer.locator('[data-component="terminal-tab"], [data-component="ai-chat-tab"]').evaluateAll(tabs=>tabs.map(tab=>(tab as HTMLElement).dataset.tabId))}

async function dragBefore(page:import('@playwright/test').Page,sourceId:string,targetId:string){await page.evaluate(({sourceId,targetId})=>{const source=document.querySelector<HTMLElement>(`[data-tab-id="${sourceId}"]`)!,target=document.querySelector<HTMLElement>(`[data-tab-id="${targetId}"]`)!,transfer=new DataTransfer(),bounds=target.getBoundingClientRect(),init={bubbles:true,cancelable:true,dataTransfer:transfer,clientX:bounds.left+1,clientY:bounds.top+bounds.height/2};source.dispatchEvent(new DragEvent('dragstart',init));target.dispatchEvent(new DragEvent('dragover',init));target.dispatchEvent(new DragEvent('drop',init));source.dispatchEvent(new DragEvent('dragend',init))},{sourceId,targetId})}

test('orders terminal and AI-chat tabs as one persistent, keyboard-accessible drawer sequence',async({page})=>{
  await page.setViewportSize({width:1440,height:900});await installFixture(page);await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();await page.getByRole('button',{name:'Show terminal drawer'}).click();const drawer=page.locator('[data-component="terminal-drawer"]');await expect(drawer).toBeVisible();await expect.poll(()=>drawerOrder(drawer)).toEqual(['shell-one','shell-two']);
  await drawer.getByRole('button',{name:'New drawer item'}).click();await drawer.getByRole('menu',{name:'New drawer item'}).getByText('AI chat').click();const chat=drawer.locator('[data-component="ai-chat-tab"]');await expect(chat).toBeVisible();const chatId=await chat.getAttribute('data-tab-id');expect(chatId).toBeTruthy();await expect.poll(()=>drawerOrder(drawer)).toEqual(['shell-one','shell-two',chatId!]);
  await dragBefore(page,chatId!,'shell-one');await expect.poll(()=>drawerOrder(drawer)).toEqual([chatId!,'shell-one','shell-two']);
  const shellTwo=drawer.locator('[data-tab-id="shell-two"] .kui-app-tab__select');await shellTwo.focus();await page.keyboard.press('Alt+Shift+ArrowLeft');await expect.poll(()=>drawerOrder(drawer)).toEqual([chatId!,'shell-two','shell-one']);await expect(shellTwo).toBeFocused();await expect(shellTwo).toHaveAttribute('aria-keyshortcuts','Delete Backspace Alt+Shift+ArrowLeft Alt+Shift+ArrowRight');
  await drawer.getByRole('button',{name:'Hide terminal drawer'}).click();await expect(drawer).not.toBeVisible();await page.getByRole('button',{name:'Show terminal drawer'}).click();await expect(drawer).toBeVisible();await expect.poll(()=>drawerOrder(drawer)).toEqual([chatId!,'shell-two','shell-one']);expect(await page.evaluate(projectId=>localStorage.getItem(`hotsheet.project.${projectId}.terminal-drawer-order`),project.id)).toBe(JSON.stringify([chatId!,'shell-two','shell-one']));
  await page.screenshot({path:'/private/tmp/hs2-rrf9ee-drawer-order-wide.png',fullPage:true});await page.setViewportSize({width:900,height:650});await expect.poll(()=>drawerOrder(drawer)).toEqual([chatId!,'shell-two','shell-one']);await page.screenshot({path:'/private/tmp/hs2-rrf9ee-drawer-order-narrow.png',fullPage:true});
  await chat.getByRole('tab').click();await chat.getByRole('tab').focus();await page.keyboard.press('Delete');await expect.poll(()=>drawerOrder(drawer)).toEqual(['shell-two','shell-one']);await expect(shellTwo).toHaveAttribute('aria-selected','true');await expect(shellTwo).toBeFocused();await page.keyboard.press('Delete');await expect.poll(()=>drawerOrder(drawer)).toEqual(['shell-one']);const shellOne=drawer.locator('[data-tab-id="shell-one"] .kui-app-tab__select');await expect(shellOne).toHaveAttribute('aria-selected','true');await expect(shellOne).toBeFocused();
});

test('right-click closes mixed terminal and AI-chat ranges from either tab kind',async({page})=>{
  await page.setViewportSize({width:1440,height:900});await installFixture(page);await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();await page.getByRole('button',{name:'Show terminal drawer'}).click();const drawer=page.locator('[data-component="terminal-drawer"]');
  for(let index=0;index<2;index+=1){await drawer.getByRole('button',{name:'New drawer item'}).click();await drawer.getByRole('menu',{name:'New drawer item'}).getByText('AI chat').click()}
  const chats=drawer.locator('[data-component="ai-chat-tab"]'),chatOneId=(await chats.nth(0).getAttribute('data-tab-id'))!,chatTwoId=(await chats.nth(1).getAttribute('data-tab-id'))!;await dragBefore(page,chatOneId,'shell-two');await expect.poll(()=>drawerOrder(drawer)).toEqual(['shell-one',chatOneId,'shell-two',chatTwoId]);
  await drawer.locator(`[data-tab-id="${chatOneId}"]`).getByRole('tab').click({button:'right'});const chatMenu=page.getByRole('menu',{name:'AI chat tab actions'});await expect(chatMenu).toBeVisible();await expect(chatMenu.getByText('Rename…')).toHaveCount(0);await chatMenu.getByText('Close Tabs to the Right').click();await expect.poll(()=>drawerOrder(drawer)).toEqual(['shell-one',chatOneId]);
  await drawer.locator('[data-tab-id="shell-one"]').getByRole('tab').click({button:'right'});const terminalMenu=page.getByRole('menu',{name:'Terminal tab actions'});await terminalMenu.getByText('Close All Tabs').click();await expect.poll(()=>drawerOrder(drawer)).toEqual([]);await expect(drawer.getByRole('tab',{name:'Project grid'})).toHaveAttribute('aria-selected','true');
  await page.screenshot({path:'/private/tmp/hs2-3kwk6m-mixed-drawer-close-wide.png',fullPage:true});await page.setViewportSize({width:900,height:650});await page.screenshot({path:'/private/tmp/hs2-3kwk6m-mixed-drawer-close-narrow.png',fullPage:true});
});
