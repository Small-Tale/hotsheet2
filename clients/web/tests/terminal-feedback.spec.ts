import { expect,test } from '@playwright/test';

const project={id:'terminal-feedback',root:'/work/terminal-feedback',name:'Terminal feedback',stores:['/work/terminal-feedback.hs2'],apiPath:'/__hotsheet/project-api/terminal-feedback'};
const capabilities={create:true,update:true,close:true,notes:true,note_edit:true,note_delete:true,attachments:true,assignment:true,review_requests:true,dependencies:true,up_next:true,close_reasons:true,claims:true,atomic_batch:true,not_working_report:true,offline_mutation:true,history:true,watch:true,provider_idempotency:true,query_fields:[]};

async function installTerminalFixture(page:import('@playwright/test').Page){
  await page.addInitScript(()=>{
    const nano=(cols=80,rows=24)=>`\u001b[2J\u001b[H  GNU nano 8.4                 terminal-fill-proof.txt\u001b[2;1H\u001b[7mFile: terminal-fill-proof.txt\u001b[0m\u001b[${Math.max(3,Math.floor(rows/2))};20H${cols} columns × ${rows} rows\u001b[${Math.max(2,rows-1)};1H\u001b[7m^G Help  ^O Write Out  ^W Where Is  ^K Cut  ^T Execute\u001b[0m\u001b[${rows};1H\u001b[7m^X Exit  ^R Read File  ^\\ Replace  ^U Paste  ^J Justify\u001b[0m`;
    const sockets:FakeSocket[]=[];
    class FakeSocket extends EventTarget{
      static CONNECTING=0;static OPEN=1;static CLOSING=2;static CLOSED=3;readyState=0;binaryType='blob';sent:unknown[]=[];
      constructor(public url:string){super();sockets.push(this);setTimeout(()=>{this.readyState=1;this.dispatchEvent(new Event('open'));this.dispatchEvent(new MessageEvent('message',{data:new TextEncoder().encode(nano()).buffer}))})}
      send(value:unknown){this.sent.push(value);if(typeof value!=='string')return;try{const resize=JSON.parse(value).resize;if(resize){this.dispatchEvent(new MessageEvent('message',{data:JSON.stringify({pty_size:{cols:resize.cols,rows:resize.rows},driven_by:resize.viewer_id})}));this.dispatchEvent(new MessageEvent('message',{data:new TextEncoder().encode(nano(resize.cols,resize.rows)).buffer}))}}catch{/* input */}}
      close(){this.readyState=3;this.dispatchEvent(new CloseEvent('close'))}
    }
    Object.assign(window,{WebSocket:FakeSocket,__terminalFeedbackSockets:sockets});
  });
  await page.route('**/*',route=>{
    const request=route.request(),path=new URL(request.url()).pathname;
    if(path==='/__hotsheet/projects/open')return route.fulfill({status:201,json:project});
    if(path.endsWith('/providers'))return route.fulfill({json:[{connection_id:'git-local',provider:'git',display_name:'Hot Sheet git',locator:'/tickets',default:true,capabilities}]});
    if(path.endsWith('/permissions')||path.endsWith('/connections')||path.endsWith('/commands')||path.endsWith('/command-runs')||path.endsWith('/tickets')||path.endsWith('/corrupt-tickets'))return route.fulfill({json:[]});
    if(path.endsWith('/ws/poll'))return route.fulfill({json:{cursor:Number(new URL(request.url()).searchParams.get('since')??0),events:[],overflow:false}});
    if(path.endsWith('/repository/status'))return route.fulfill({json:{branch:'main',ahead:0,behind:0,staged:0,unstaged:0,untracked:0,conflicted:0,clean:true}});
    if(path.endsWith('/terminals'))return route.fulfill({json:[{id:'nano',alive:true,busy:true,cwd:project.root,progress:50}]});
    if(path.endsWith('/terminals/nano'))return route.fulfill({json:{id:'nano',alive:true,busy:true,cwd:project.root,progress:50,scrollback:'GNU nano 8.4\n80 columns × 24 rows\n^X Exit'}});
    return route.continue();
  });
}

test('fills fixed 80 by 24 Nano grids and keeps the dedicated drawer contained',async({page})=>{
  await page.setViewportSize({width:1440,height:1100});await installTerminalFixture(page);await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();await page.getByRole('button',{name:'Terminal dashboard'}).click();
  const dashboard=page.getByRole('region',{name:'Terminal dashboard'}),tile=dashboard.locator('[data-terminal-key="terminal-feedback:nano"]'),preview=tile.locator('[data-display-mode="scaled-preview"]');await expect(preview).toHaveAttribute('data-grid-size','80x24');await expect(preview.locator('.xterm-rows')).toContainText('GNU nano');await expect(preview.locator('.xterm-rows')).toContainText('Exit');await expect(page.locator('wa-select[name="terminal-grouping"]')).toHaveCount(0);await expect.poll(()=>preview.evaluate(element=>{const screen=element.querySelector<HTMLElement>('.xterm-screen')?.getBoundingClientRect(),box=element.getBoundingClientRect();return screen?Math.max(Math.abs(screen.width-box.width),Math.abs(screen.height-box.height)):999})).toBeLessThanOrEqual(2);await page.screenshot({path:'/private/tmp/hs2-6gamwq-fixed-grid-wide.png',fullPage:true});for(let step=0;step<3;step+=1)await dashboard.getByRole('button',{name:/Zoom in/}).click();await expect(dashboard).toHaveAttribute('data-fit','1');await page.screenshot({path:'/private/tmp/hs2-6gamwq-fixed-grid-fit-one.png',fullPage:true});
  await tile.click();const magnified=dashboard.getByRole('dialog',{name:'Magnified nano'}),magnifiedViewport=magnified.locator('[data-display-mode="interactive"]');await expect(magnifiedViewport).toHaveAttribute('data-grid-size','80x24');await expect(magnifiedViewport.locator('.xterm-rows')).toContainText('Exit');await expect.poll(()=>magnifiedViewport.evaluate(element=>{const screen=element.querySelector<HTMLElement>('.xterm-screen')?.getBoundingClientRect(),box=element.getBoundingClientRect();return screen?Math.max(Math.abs(screen.width-box.width),Math.abs(screen.height-box.height)):999})).toBeLessThanOrEqual(2);await page.screenshot({path:'/private/tmp/hs2-xxxhny-fixed-grid-magnified.png',fullPage:true});
  await magnified.getByRole('button',{name:'Open nano in project terminal drawer'}).click();const drawer=page.locator('[data-component="terminal-drawer"]');await expect(drawer).toHaveAttribute('data-mode','dedicated');await expect(drawer.getByRole('button',{name:'Manage terminal visibility'})).toHaveCount(0);await drawer.locator('.terminal-drawer__rail').dispatchEvent('dblclick');await expect(drawer).toHaveAttribute('data-maximized','true');const dedicatedInput=drawer.getByRole('textbox',{name:'Terminal input'});await expect(dedicatedInput).toBeVisible();await expect(dedicatedInput.evaluate(node=>getComputedStyle(node.closest<HTMLElement>('.terminal-viewport')!).overflow)).resolves.toBe('hidden');await page.screenshot({path:'/private/tmp/hs2-hpjb1k-dedicated-nano-contained.png',fullPage:true});
  await drawer.getByRole('tab',{name:'Terminal grid'}).click();await expect(drawer).toHaveAttribute('data-mode','grid');await expect(drawer.locator('wa-select[name="terminal-visibility-group"]')).toHaveCount(0);await page.screenshot({path:'/private/tmp/hs2-p0pyh7-drawer-without-visibility.png',fullPage:true});
});
