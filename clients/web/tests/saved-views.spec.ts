import {expect,test} from '@playwright/test';

const project={id:'saved-views',root:'/work/saved-views',name:'Saved views',stores:['/work/saved-views.hs2'],apiPath:'/__hotsheet/project-api/saved-views'};
const base={connection_id:'git',native_id:'1',qualified_id:'git:1',id:'1',slug:'HS2-DOCS',title:'Document the saved view',category:'task',priority:'default',status:'not_started',up_next:true,feedback_needed:false,tags:['docs'],blocked_by:[],claim_count:0,created_at:'2026-09-10T00:00:00Z',updated_at:'2026-09-10T00:00:00Z'};
const rows=[base,{...base,native_id:'2',qualified_id:'git:2',id:'2',slug:'HS2-CODE',title:'Implement the parser',tags:['client']}];

test('creates, applies, and shares a custom ticket view',async({page})=>{
  let views:Array<{id:string;name:string;query:string}>=[];
  await page.route('**/*',route=>{const request=route.request(),url=new URL(request.url()),path=url.pathname;
    if(path==='/__hotsheet/projects/open')return route.fulfill({status:201,json:project});
    if(path.endsWith('/views')&&request.method()==='PUT'){views=request.postDataJSON();return route.fulfill({json:views})}
    if(path.endsWith('/views'))return route.fulfill({json:views});
    if(path.endsWith('/tickets')&&request.method()==='GET')return route.fulfill({json:rows});
    if(path.endsWith('/providers'))return route.fulfill({json:[{connection_id:'git-local',provider:'git',display_name:'Hot Sheet git',locator:project.stores[0],default:true,capabilities:{create:true,update:true,notes:true,attachments:true,watch:true,query_fields:[]}}]});
    if(path.endsWith('/connections')||path.endsWith('/permissions')||path.endsWith('/commands')||path.endsWith('/command-runs')||path.endsWith('/terminals')||path.endsWith('/corrupt-tickets'))return route.fulfill({json:[]});
    if(path.endsWith('/repository/status'))return route.fulfill({json:{branch:'main',ahead:0,behind:0,staged:0,unstaged:0,untracked:0,conflicted:0,clean:true}});
    if(path.endsWith('/ws/poll'))return route.fulfill({json:{cursor:Number(url.searchParams.get('since')??0),events:[],overflow:false}});
    return route.continue();
  });
  await page.setViewportSize({width:1440,height:900});await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();
  await page.getByRole('button',{name:'Add view'}).click();const dialog=page.locator('[data-component="saved-view-dialog"]');await expect(dialog.getByRole('textbox',{name:'View name'})).toBeVisible();await dialog.getByRole('textbox',{name:'View name'}).fill('Needs docs');await dialog.getByRole('textbox',{name:'Search query'}).fill('tag:docs');await dialog.getByRole('button',{name:'Create View'}).click();
  await expect(page.getByRole('button',{name:/Needs docs/})).toHaveAttribute('aria-current','page');await expect(page.getByRole('heading',{name:'Needs docs'})).toBeVisible();await expect(page.locator('[data-ticket-slug="HS2-DOCS"]')).toBeVisible();await expect(page.locator('[data-ticket-slug="HS2-CODE"]')).toHaveCount(0);expect(views).toEqual([{id:'needs-docs',name:'Needs docs',query:'tag:docs'}]);
  await page.screenshot({path:'/private/tmp/hs2-f34p38-custom-view-wide.png',fullPage:true});await page.setViewportSize({width:720,height:760});await page.screenshot({path:'/private/tmp/hs2-f34p38-custom-view-narrow.png',fullPage:true});
  await page.getByRole('button',{name:/Queue/}).click();await expect(page.locator('[data-ticket-slug="HS2-CODE"]')).toBeVisible();await expect(page.getByRole('button',{name:'Search tickets'})).toBeVisible();
});
