import {expect,test} from '@playwright/test';

// Mobile single-column layout with overlay sidebars (HS2-ZK51WP): below the desktop size floor the
// project sidebar and ticket inspector overlay the single main column, only one is open at a time,
// and a click-away scrim dismisses whichever is open.

const capabilities={create:true,update:true,close:true,notes:true,note_edit:true,note_delete:true,attachments:true,assignment:true,review_requests:true,dependencies:true,up_next:true,close_reasons:true,claims:true,atomic_batch:true,not_working_report:true,offline_mutation:true,history:true,watch:true,provider_idempotency:true,query_fields:[]};
const project=(id:string,root:string)=>({id,root,name:root.split('/').at(-1),stores:[`${root}.hs2`],apiPath:`/__hotsheet/project-api/${id}`});
const ticket=(slug:string,status:string)=>({connection_id:'git-local',native_id:slug,qualified_id:`git-local:${slug}`,id:slug,slug,title:slug,status,up_next:false,feedback_needed:false,tags:[],blocked_by:[],claim_count:0});

async function openDemoProject(page:import('@playwright/test').Page){
  await page.route('**/*',async route=>{
    const request=route.request(),url=new URL(request.url()),path=url.pathname;
    if(path==='/__hotsheet/projects/open')return route.fulfill({status:201,json:project('demo-checkout',request.postDataJSON().root as string)});
    if(path==='/__hotsheet/folders/choose')return route.fulfill({json:{path:'/work/demo'}});
    if(path.endsWith('/providers'))return route.fulfill({json:[{connection_id:'git-local',provider:'git',display_name:'Hot Sheet git',locator:'/tickets',default:true,capabilities}]});
    // Flattened ticket + store, matching GET /checkouts/{ref}/tickets/{id} (the client wraps it itself).
    if(/\/tickets\/HS2-M1$/.test(path))return route.fulfill({json:{store:'git-local',...ticket('HS2-M1','started'),category:'bug',priority:'default',details:'',blocked_reason:null,notes:[],attachments:[],created_at:'2026-09-01T00:00:00Z',updated_at:'2026-09-14T00:00:00Z',concurrency_token:'t1'}});
    if(path.endsWith('/tickets'))return route.fulfill({json:{items:[ticket('HS2-M1','started')],counts:{total:1,queued:1,backlog:0,archive:0,open:1,up_next:0,active:0,started:1,completed_today:0,completion_trend:[0,0,0,0,0,0,0]}}});
    if(path.endsWith('/repository/status'))return route.fulfill({json:{branch:'main',ahead:0,behind:0,staged:0,unstaged:0,untracked:0,conflicted:0,clean:true}});
    if(path.endsWith('/ws/poll'))return route.fulfill({json:{cursor:Number(url.searchParams.get('since')??0),events:[],overflow:false}});
    if(path.endsWith('/terminals')||path.endsWith('/connections')||path.endsWith('/commands')||path.endsWith('/command-runs')||path.endsWith('/views')||path.endsWith('/corrupt-tickets'))return route.fulfill({json:[]});
    return route.continue();
  });
  await page.goto('/');await page.getByRole('button',{name:'Open project'}).click();await page.getByRole('button',{name:'Open project',exact:true}).last().click();
}

test('mobile viewport uses a single-column layout with overlay sidebars, one at a time (HS2-ZK51WP)',async({page})=>{
  await page.setViewportSize({width:390,height:844});
  await openDemoProject(page);
  const shell=page.locator('[data-component="app-shell"]');
  await expect(shell).toHaveAttribute('data-mobile','true');
  const sidebar=page.locator('.kui-resizable-region[data-region-id="app-sidebar"]');
  const inspector=page.locator('.kui-resizable-region[data-region-id="app-inspector"]');
  const scrim=page.locator('.app-shell__scrim');
  // Single column: both overlays start closed and the scrim is absent.
  await expect(sidebar).toHaveAttribute('data-collapsed','true');
  await expect(inspector).toHaveAttribute('data-collapsed','true');
  await expect(scrim).toHaveCount(0);

  // Open the project sidebar as an overlay.
  await page.getByRole('button',{name:'Show project sidebar'}).click();
  await expect(sidebar).toHaveAttribute('data-collapsed','false');
  await expect(scrim).toBeVisible();
  // The sidebar overlays the main column rather than sitting beside it.
  await expect(sidebar).toHaveCSS('position','absolute');

  await page.screenshot({path:'/private/tmp/hs2-zk51wp-mobile-sidebar-overlay.png',fullPage:true});

  // Click-away scrim dismisses the open sidebar and returns to a single column. (The scrim covers
  // the toolbar, so switching overlays is dismiss-then-open — normal mobile drawer behavior.) Click
  // the scrim strip on the side the left sidebar does not cover.
  await scrim.click({position:{x:370,y:400}});
  await expect(sidebar).toHaveAttribute('data-collapsed','true');
  await expect(scrim).toHaveCount(0);

  // The inspector opens as its own overlay while the sidebar stays closed (one at a time).
  await page.getByRole('button',{name:'Show ticket inspector'}).click();
  await expect(inspector).toHaveAttribute('data-collapsed','false');
  await expect(sidebar).toHaveAttribute('data-collapsed','true');
  await expect(scrim).toBeVisible();
  await page.screenshot({path:'/private/tmp/hs2-zk51wp-mobile-inspector-overlay.png',fullPage:true});

  // Dismiss via the scrim strip the right inspector does not cover.
  await scrim.click({position:{x:10,y:400}});
  await expect(inspector).toHaveAttribute('data-collapsed','true');
  await expect(scrim).toHaveCount(0);
  await page.screenshot({path:'/private/tmp/hs2-zk51wp-mobile-single-column.png',fullPage:true});
});

test('mobile forces list view and hides the columns toggle, restoring board view on desktop (HS2-1XCHZT)',async({page})=>{
  await page.setViewportSize({width:1280,height:800});
  await openDemoProject(page);
  await expect(page.locator('[data-ticket-slug="HS2-M1"]')).toBeVisible();
  // Desktop: the columns/board toggle is available and switches to a board.
  await page.getByRole('button',{name:'Columns view'}).click();
  await expect(page.locator('[data-component="ticket-board"]')).toBeVisible();
  await expect(page.locator('[data-component="ticket-list"]')).toHaveCount(0);

  // Shrinking below the floor forces the list view and removes the columns toggle entirely.
  await page.setViewportSize({width:390,height:844});
  await expect(page.getByRole('button',{name:'Columns view'})).toHaveCount(0);
  await expect(page.locator('[data-component="ticket-list"]')).toBeVisible();
  await expect(page.locator('[data-component="ticket-board"]')).toHaveCount(0);
  await expect(page.getByRole('button',{name:'List view'})).toHaveAttribute('aria-pressed','true');

  // Growing back restores the desktop board preference (it was never overwritten).
  await page.setViewportSize({width:1280,height:800});
  await expect(page.locator('[data-component="ticket-board"]')).toBeVisible();
  await expect(page.getByRole('button',{name:'Columns view'})).toBeVisible();
});

test('tapping a ticket auto-opens the inspector overlay, and tap-away returns to the list (HS2-N7RPFP)',async({page})=>{
  await page.setViewportSize({width:390,height:844});
  await openDemoProject(page);
  const inspector=page.locator('.kui-resizable-region[data-region-id="app-inspector"]');
  const scrim=page.locator('.app-shell__scrim');
  await expect(page.locator('[data-ticket-slug="HS2-M1"]')).toBeVisible();
  // Inspector starts closed on mobile.
  await expect(inspector).toHaveAttribute('data-collapsed','true');

  // Tapping the ticket row auto-opens the inspector overlay and shows the ticket.
  await page.locator('[data-action="select-ticket-row"][data-ticket-slug="HS2-M1"]').click();
  await expect(inspector).toHaveAttribute('data-collapsed','false');
  await expect(scrim).toBeVisible();
  await expect(page.locator('[data-component="ticket-inspector"]')).toBeVisible();

  // Tap-away on the scrim returns to the list; the selection persists so tapping reopens it.
  await scrim.click({position:{x:10,y:400}});
  await expect(inspector).toHaveAttribute('data-collapsed','true');
  await expect(scrim).toHaveCount(0);
  await page.locator('[data-action="select-ticket-row"][data-ticket-slug="HS2-M1"]').click();
  await expect(inspector).toHaveAttribute('data-collapsed','false');
});

test('resizing from mobile back to desktop restores the side-by-side layout (HS2-ZK51WP)',async({page})=>{
  await page.setViewportSize({width:390,height:844});
  await openDemoProject(page);
  const shell=page.locator('[data-component="app-shell"]');
  const sidebar=page.locator('.kui-resizable-region[data-region-id="app-sidebar"]');
  await page.getByRole('button',{name:'Show project sidebar'}).click();
  await expect(sidebar).toHaveAttribute('data-collapsed','false');
  // Growing past the breakpoint drops mobile mode and its ephemeral open state.
  await page.setViewportSize({width:1280,height:800});
  await expect(shell).toHaveAttribute('data-mobile','false');
  await expect(sidebar).not.toHaveCSS('position','absolute');
  // Desktop default has the sidebar visible in-flow.
  await expect(sidebar).toHaveAttribute('data-collapsed','false');
});
