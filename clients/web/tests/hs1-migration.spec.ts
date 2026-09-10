import {expect,test} from '@playwright/test';

test('imports an HS1 project, then offers cleanup only after remote backup',async({page})=>{
  let imported=false,remote=false,deleted=false,providerRequests=0;
  await page.route('**/*',async route=>{
    const request=route.request(),path=new URL(request.url()).pathname;
    if(path==='/__hotsheet/projects/open')return route.fulfill({status:201,json:{id:'legacy',root:'/work/legacy',name:'Legacy project',stores:[],apiPath:'/__hotsheet/project-api/legacy',needsTicketSetup:true,needsHs1Migration:true,hs1ImportCompleted:false,hs1CleanupEligible:false}});
    if(path==='/__hotsheet/projects/migrate-hs1'&&request.method()==='POST'){expect(request.postDataJSON()).toEqual({root:'/work/legacy',location:'/work/legacy.hs2'});imported=true;return route.fulfill({status:201,json:{ticketStore:'/work/legacy.hs2',connectionId:'git-import',tickets:27,attachments:4,toolsConfigured:true}})}
    if(path.includes('/sources/')&&request.method()==='PUT')return route.fulfill({json:{id:'legacy',root:'/work/legacy',alias:'Legacy project',stores:['/work/legacy.hs2'],sources:[{connection_id:'git-import',provider:'git',locator:'/work/legacy.hs2'}],default_source:'git-import'}});
    if(path==='/__hotsheet/projects/setup-git-remote'&&request.method()==='POST'){remote=true;return route.fulfill({json:{connected:true}})}
    if(path==='/__hotsheet/projects/legacy/hs1-data'&&request.method()==='DELETE'){deleted=true;return route.fulfill({json:{removed:['db','attachments','settings.json']}})}
    if(path.endsWith('/providers')){providerRequests+=1;return route.fulfill({json:imported?[{connection_id:'git-import',provider:'git',display_name:'Imported Hot Sheet 1 tickets',locator:'/work/legacy.hs2',default:true,capabilities:{create:true,update:true,atomic_batch:true,notes:true,attachments:true,watch:true,query_fields:[]}}]:[]})}
    if(path.endsWith('/tickets')||path.endsWith('/corrupt-tickets')||path.endsWith('/connections')||path.endsWith('/permissions')||path.endsWith('/commands')||path.endsWith('/terminals'))return route.fulfill({json:[]});
    if(path.endsWith('/terminal-settings'))return route.fulfill({json:{inherit_global_shell_history:false}});
    if(path.endsWith('/repository/status'))return route.fulfill({json:{branch:'main',ahead:0,behind:0,staged:0,unstaged:0,untracked:0,conflicted:0,clean:true}});
    if(path.endsWith('/ws/poll'))return route.fulfill({json:{cursor:0,events:[],overflow:false}});
    return route.continue();
  });
  await page.setViewportSize({width:1100,height:800});await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();
  const dialog=page.locator('[data-component="hs1-migration-dialog"]');await expect(dialog).toHaveJSProperty('open',true);await expect(dialog).toContainText('tickets, notes, attachments, settings, and AI-tool setup');await expect(dialog.getByRole('textbox',{name:'Ticket repository folder'})).toHaveValue('/work/legacy.hs2');
  await dialog.evaluate(async node=>{await Promise.all(node.getAnimations({subtree:true}).map(animation=>animation.finished))});
  await page.screenshot({path:'/private/tmp/hs2-pwyts8-hs1-import-wide.png',fullPage:true});await page.setViewportSize({width:520,height:720});await expect.poll(()=>page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth)).toBe(true);await page.screenshot({path:'/private/tmp/hs2-pwyts8-hs1-import-narrow.png',fullPage:true});
  await dialog.getByRole('button',{name:'Import project'}).click();await expect(page.locator('[data-ticket-source-setup-dialog]')).toHaveJSProperty('open',true);await expect(page.locator('.app-toast')).toContainText('Imported 27 tickets and 4 attachments');expect(providerRequests).toBeGreaterThanOrEqual(2);await page.getByRole('textbox',{name:'Remote URL'}).fill('git@example.com:team/legacy.hs2.git');await page.getByRole('button',{name:'Connect & push'}).click();expect(remote).toBe(true);
  const banner=page.locator('[data-component="hs1-cleanup-banner"]');await expect(banner).toBeVisible();await expect(banner).toContainText('safely backed up');await page.setViewportSize({width:1100,height:800});await page.screenshot({path:'/private/tmp/hs2-pwyts8-hs1-cleanup-banner.png',fullPage:true});
  page.once('dialog',prompt=>prompt.accept());await banner.getByRole('button',{name:'Delete old files…'}).click();await expect(banner).toHaveCount(0);expect(deleted).toBe(true);
});
