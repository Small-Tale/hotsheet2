import {expect,test} from '@playwright/test';

const project={id:'no-git',root:'/work/no-git',name:'No Git project',stores:['/work/no-git.hs2'],apiPath:'/__hotsheet/project-api/no-git'};

test('recovers a project folder without Git and optionally connects origin',async({page})=>{
  let initialized=false,initBody:string|null|undefined,remoteBody:unknown;
  await page.route('**/*',route=>{const request=route.request(),url=new URL(request.url()),path=url.pathname;
    if(path==='/__hotsheet/projects/open')return route.fulfill({status:201,json:project});
    if(path.endsWith('/repository/init')&&request.method()==='POST'){initBody=request.postData();initialized=true;return route.fulfill({json:{initialized:true,branch:'master',ahead:0,behind:0,staged:0,unstaged:0,untracked:1,conflicted:0,clean:false,root:project.root,platform:'macos',commit_count:0,commits:[],ranges:[],files:[],truncated:false}})}
    if(path.endsWith('/repository/remote')&&request.method()==='POST'){remoteBody=request.postDataJSON();return route.fulfill({json:{initialized:true,branch:'master',ahead:0,behind:0,staged:0,unstaged:0,untracked:1,conflicted:0,clean:false,root:project.root,platform:'macos',commit_count:0,commits:[],ranges:[],files:[],truncated:false}})}
    if(path.endsWith('/repository/status'))return route.fulfill({json:initialized?{initialized:true,branch:'master',ahead:0,behind:0,staged:0,unstaged:0,untracked:1,conflicted:0,clean:false,root:project.root,platform:'macos',commit_count:0,commits:[],ranges:[],files:[],truncated:false}:{initialized:false,ahead:0,behind:0,staged:0,unstaged:0,untracked:0,conflicted:0,clean:true,root:project.root,platform:'macos',commit_count:0,commits:[],ranges:[],files:[],truncated:false}});
    if(path.endsWith('/repository/files'))return route.fulfill({json:{items:[{path:'existing.txt',unstaged:'untracked',untracked:true,conflicted:false}],next_cursor:null}});
    if(path.endsWith('/repository/commits'))return route.fulfill({json:{items:[],next_cursor:null}});
    if(path.endsWith('/tickets'))return route.fulfill({json:url.searchParams.has('page_size')?{items:[],counts:{total:0,queued:0,backlog:0,archive:0,open:0,up_next:0,active:0,started:0,completed_today:0}}:[]});
    if(path.endsWith('/providers'))return route.fulfill({json:[{connection_id:'git-local',provider:'git',display_name:'Hot Sheet git',locator:project.stores[0],default:true,capabilities:{create:true,update:true,notes:true,attachments:true,watch:true,query_fields:[]}}]});
    if(path.endsWith('/views')||path.endsWith('/connections')||path.endsWith('/permissions')||path.endsWith('/commands')||path.endsWith('/command-runs')||path.endsWith('/terminals')||path.endsWith('/corrupt-tickets')||path.endsWith('/drive/sessions'))return route.fulfill({json:[]});
    if(path.endsWith('/ai-tools'))return route.fulfill({json:[]});
    if(path.endsWith('/ai-settings'))return route.fulfill({json:{tool:'codex'}});
    if(path.endsWith('/terminal-settings'))return route.fulfill({json:{inherit_global_shell_history:false}});
    if(path.endsWith('/ws/poll'))return route.fulfill({json:{cursor:Number(url.searchParams.get('since')??0),events:[],overflow:false}});
    return route.continue();
  });

  await page.setViewportSize({width:1440,height:900});
  await page.goto('/');
  await page.getByRole('button',{name:'Open project'}).click();
  await page.getByRole('button',{name:'Open project',exact:true}).last().click();
  await page.locator('[data-action="open-repository-status"]').click();
  const dialog=page.locator('[data-component="repository-status-popover"]');
  await expect(dialog).toHaveAttribute('data-state','uninitialized');
  await expect(dialog.getByRole('heading',{name:'This folder is not a Git repository'})).toBeVisible();
  await expect(dialog.getByRole('heading',{name:'Repository Status'})).toHaveCount(0);
  const initialize=dialog.locator('[data-action="initialize-repository"]');await expect.poll(()=>initialize.evaluate(node=>{const button=node.getBoundingClientRect(),footer=node.closest('footer')!.getBoundingClientRect();return Math.max(Math.abs(button.x+button.width/2-footer.x-footer.width/2),Math.abs(button.y+button.height/2-footer.y-footer.height/2))})).toBeLessThan(1);
  await expect(dialog.getByText(/will not stage or commit/)).toBeVisible();
  await dialog.screenshot({path:'/private/tmp/hs2-9r3w53-no-git-wide.png'});
  await page.setViewportSize({width:720,height:640});
  await dialog.screenshot({path:'/private/tmp/hs2-9r3w53-no-git-narrow.png'});

  await dialog.getByRole('button',{name:'Initialize Git repository'}).click();
  await expect(dialog.getByText('Git is ready',{exact:true})).toBeVisible();
  expect(initBody).toBeNull();
  await dialog.getByRole('textbox',{name:'Remote URL'}).fill('git@example.com:team/project.git');
  await dialog.getByRole('button',{name:'Add origin'}).click();
  await expect(dialog.locator('[data-step="remote"]')).toHaveCount(0);
  await dialog.getByRole('button',{name:/Untracked 1/}).click();
  await expect(dialog.getByText('existing.txt')).toBeVisible();
  expect(remoteBody).toEqual({remote:'git@example.com:team/project.git'});
});
