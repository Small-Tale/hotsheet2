import { expect, test } from '@playwright/test';

import { expectResponsiveFeedbackRectangle, measureFeedbackRectangle } from './dev-review-performance';

test('represents every advanced-search component with the real responsive overlay',async({page})=>{
  await page.setViewportSize({width:1440,height:900});await page.goto('/ux-demo?component=global-search');const overlay=page.locator('[data-component="global-search-overlay"]');await expect(overlay).toBeVisible();for(const component of ['active-filter-bar','filter-chip','select','menu-item'])await expect(overlay.locator(`[data-component="${component}"]`).first()).toBeVisible();await expect(overlay.getByText('Ticket reference',{exact:true})).toBeVisible();await page.screenshot({path:'/private/tmp/hs2-383d6k-ux-search-wide.png',fullPage:true});
  await page.setViewportSize({width:390,height:844});await expect(overlay).toBeVisible();await page.screenshot({path:'/private/tmp/hs2-383d6k-ux-search-narrow.png',fullPage:true});
});

test('navigates the catalog and preserves URL-addressable selection', async ({ page }) => {
  await page.goto('/ux-demo');
  await expect(page.getByRole('heading', { name: 'UX components' })).toBeVisible();
  const reviewToggle = page.getByRole('button', { name: 'Dev Review On' });
  await expect(page.locator('.hs-dev-review')).toBeVisible();
  await reviewToggle.click();
  await expect(page).toHaveURL('/ux-demo?dev-review=false');
  await expect(page.getByRole('button', { name: 'Dev Review Off' })).toBeVisible();
  await expect(page.locator('.hs-dev-review')).toHaveCount(0);
  const catalog = page.getByRole('navigation');
  await expect(catalog.locator('[data-item-id="global-search"]')).not.toHaveCSS('color', 'rgb(174, 174, 178)');
  await expect(catalog.locator('[data-item-id="app-shell"]')).not.toHaveCSS('color', 'rgb(174, 174, 178)');
  await expect(catalog.locator('[data-item-id="ticket-row"]')).not.toHaveCSS('color', 'rgb(174, 174, 178)');
  await expect(catalog.locator('[data-component="menu-header"]')).not.toHaveCount(0);
  await expect(catalog.locator('[data-component="menu-item"]')).not.toHaveCount(0);
  const firstCatalogList = catalog.locator('.catalog-group ul').first();
  const firstCatalogItem = firstCatalogList.locator('li').first();
  const [listBox, itemBox] = await Promise.all([firstCatalogList.boundingBox(), firstCatalogItem.boundingBox()]);
  expect(itemBox!.x).toBeCloseTo(listBox!.x, 0);
  await expect(page.getByRole('heading', { name: 'TagChip', exact: true })).toBeVisible();
  await page.getByRole('navigation').getByRole('button', { name: /TicketRow/ }).click();
  await expect(page).toHaveURL('/ux-demo?dev-review=false&component=ticket-row');
  await expect(page.getByRole('heading', { name: 'TicketRow', exact: true })).toBeVisible();
  await expect(page.getByRole('region', { name: 'TicketRow demo' })).toBeVisible();
  const catalogTop = await page.getByRole('complementary', { name: 'Component catalog' }).evaluate(node => node.getBoundingClientRect().top);
  await page.evaluate(() => { window.scrollTo(0, 500); });
  await expect.poll(() => page.getByRole('complementary', { name: 'Component catalog' }).evaluate(node => node.getBoundingClientRect().top)).toBeCloseTo(catalogTop, 0);
  await expect(page.getByRole('complementary', { name: 'Component catalog' }).getByText('Uses')).toHaveCount(0);
  const relationships = page.locator('.demo-relationships');
  await expect(relationships).toBeVisible();
  await expect(relationships.locator('.select__group').nth(0)).toHaveAttribute('aria-label', 'Used by');
  await expect(relationships.locator('.select__group').nth(1)).toHaveAttribute('aria-label', 'Uses');
  await expect(relationships.locator('.select__group').nth(1)).toHaveClass(/select__group--separated/);
  await expect(relationships.locator('wa-option', { hasText: 'TagChip' })).toHaveCount(1);
  await relationships.evaluate((node: HTMLElement & { value: string }) => { node.value = 'tag-chip'; node.dispatchEvent(new Event('change', { bubbles: true })); });
  await expect(page).toHaveURL('/ux-demo?dev-review=false&component=tag-chip');
  await expect(page.locator('.demo-relationships .select__group').nth(0)).toHaveAttribute('aria-label', 'Used by');
  await expect(page.locator('.demo-relationships wa-option', { hasText: 'TicketRow' })).toHaveCount(1);
  await page.locator('.demo-relationships').evaluate((node: HTMLElement & { value: string }) => { node.value = 'ticket-row'; node.dispatchEvent(new Event('change', { bubbles: true })); });
  await page.goBack();
  await expect(page.getByRole('heading', { name: 'TagChip', exact: true })).toBeVisible();
});

test('reopens dialog demos and keeps Feedback above the modal top layer',async({page})=>{
  await page.setViewportSize({width:1280,height:800});
  await page.goto('/ux-demo?component=hs1-migration-dialog');
  const migration=page.locator('[data-component="hs1-migration-dialog"]');
  await expect(migration).toHaveJSProperty('open',true);
  const feedback=page.locator('.hs-dev-review__feedback');
  await expect(feedback).toBeVisible();
  expect(await feedback.evaluate(node=>{const box=node.getBoundingClientRect(),top=document.elementFromPoint(box.x+box.width/2,box.y+box.height/2);return top===node||Boolean(top?.closest('.hs-dev-review__feedback'))})).toBe(true);
  await migration.getByRole('button',{name:'Not now'}).click();
  await expect(migration).toHaveJSProperty('open',false);
  await page.getByRole('button',{name:'Open import dialog'}).click();
  await expect(migration).toHaveJSProperty('open',true);
  await page.mouse.move(1000,700);
  await page.waitForTimeout(200);
  await page.screenshot({path:'/private/tmp/hs2-9a6ssk-dialog-reopened-wide.png',fullPage:true});
  await page.setViewportSize({width:390,height:844});
  await expect(feedback).toBeVisible();
  await page.screenshot({path:'/private/tmp/hs2-9a6ssk-dialog-feedback-narrow.png',fullPage:true});
});

test('represents the shared repository-status composition in the UX catalog',async({page})=>{
  await page.setViewportSize({width:1280,height:900});await page.goto('/ux-demo?component=repository-status-popover');const dialog=page.locator('[data-component="repository-status-popover"]');await expect(dialog).toBeVisible();const compare=dialog.getByRole('button',{name:'Compare two commits'}),refresh=dialog.getByRole('button',{name:'Refresh repository status'});expect(await refresh.evaluate(node=>getComputedStyle(node).color)).toBe(await compare.evaluate(node=>getComputedStyle(node).color));await expect(dialog).toHaveAttribute('data-embedded','true');await expect(dialog.locator('[data-component="menu-header"]')).toContainText('Views');await expect(dialog.locator('[data-component="menu-item"]')).not.toHaveCount(0);await page.locator('[data-action="toggle-settings"]').click();const inspector=page.getByRole('complementary',{name:'RepositoryStatusPopover settings'}),scenario=inspector.locator('wa-select[name="scenario"]');for(const [value,state,copy,icon] of [['clean','clean','Working tree is clean','circle-check'],['dirty','dirty','Local changes have not been committed','git-branch'],['ahead','ahead','Local commits have not been pushed','git-branch'],['behind','behind','Remote commits have not been integrated','git-branch'],['diverged','diverged','Local and remote histories have diverged','git-branch'],['conflicted','conflicted','Repository has unresolved conflicts','triangle-alert'],['error','error','Repository status is unavailable','triangle-alert']] as const){await scenario.evaluate((node:HTMLElement&{value:string},next)=>{node.value=next;node.dispatchEvent(new Event('change',{bubbles:true}))},value);await expect(dialog).toHaveAttribute('data-state',state);await expect(dialog.locator('header p')).toHaveText(copy);await expect(dialog.locator(`.repository-status-popover__icon [data-lucide="${icon}"]`)).toHaveCount(1)}await inspector.getByRole('button',{name:'Reset'}).click();await expect(scenario).toHaveJSProperty('value','conflicted');await expect(dialog).toHaveAttribute('data-state','conflicted');await page.screenshot({path:'/private/tmp/hs2-s6f817-repository-scenario-settings.png',fullPage:true});await page.locator('[data-action="toggle-settings"]').click();await dialog.getByRole('button',{name:/Staged 2/}).click();await expect(dialog).toHaveAttribute('data-view','staged');const file=dialog.locator('[data-action="open-repository-file-menu"]').first();await file.click();await expect(dialog.getByRole('menuitem',{name:'Show Diff'})).toBeVisible();await dialog.getByRole('menuitem',{name:'Show Diff'}).click();await expect(page.locator('.component-stage__event')).toContainText('Would review');await file.dblclick();await expect(page.locator('.component-stage__event')).toContainText('Would open');await dialog.screenshot({path:'/private/tmp/hs2-s6f817-repository-scenarios-wide.png'});await dialog.getByRole('button',{name:/Commits 24/}).click();await dialog.getByRole('button',{name:'Compare two commits'}).click();const rows=dialog.locator('.ticket-code-review__commit-summary');await rows.nth(1).click();await rows.nth(0).click();await expect(dialog.getByRole('button',{name:'Open comparison in Glassbox'})).toBeEnabled();await dialog.screenshot({path:'/private/tmp/hs2-qe4j38-repository-comparison.png'});await page.setViewportSize({width:760,height:640});await dialog.screenshot({path:'/private/tmp/hs2-z0tsx4-repository-status-demo-narrow.png'});
});

test('represents the ticket change-evidence master-detail dialog in the UX catalog',async({page})=>{
  await page.setViewportSize({width:1280,height:900});await page.goto('/ux-demo?component=change-evidence-dialog');const dialog=page.locator('[data-component="change-evidence-dialog"]');await expect(dialog).toBeVisible();await expect(dialog).toHaveAttribute('data-embedded','true');await expect(dialog).toHaveAttribute('data-view','tests');await expect(dialog.getByRole('button',{name:/Tests 1/})).toHaveAttribute('aria-current','page');const file=dialog.locator('[data-action="open-ticket-file-menu"]');await expect(file).toHaveAttribute('data-item-id','clients/web/src/change-evidence.test.ts');await file.click();await expect(dialog.getByRole('menuitem',{name:'Show Diff'})).toBeVisible();await dialog.getByRole('menuitem',{name:'Show Diff'}).click();await expect(page.locator('.component-stage__event')).toContainText('Would review');await dialog.getByRole('button',{name:/Source 1/}).click();await expect(dialog).toHaveAttribute('data-view','source');await dialog.screenshot({path:'/private/tmp/hs2-s7x4sb-change-evidence-demo-wide.png'});await page.setViewportSize({width:760,height:640});await dialog.screenshot({path:'/private/tmp/hs2-s7x4sb-change-evidence-demo-narrow.png'});
});

test('keeps repository file action menus visible at wide and narrow sizes',async({page})=>{
  await page.setViewportSize({width:1280,height:900});
  await page.goto('/ux-demo?component=repository-status-popover');
  const repository=page.locator('[data-component="repository-status-popover"]');
  await repository.getByRole('button',{name:/Staged 2/}).click();
  await repository.locator('[data-action="open-repository-file-menu"]').first().click();
  const repositoryMenu=repository.getByRole('menu');
  await expect(repositoryMenu.getByRole('menuitem').allTextContents()).resolves.toEqual(['Show Diff','Open','Show in Finder','Copy Relative Path','Copy Absolute Path']);
  await repository.screenshot({path:'/private/tmp/hs2-jgfm53-repository-file-menu-wide.png'});
  await page.setViewportSize({width:760,height:640});
  await repository.locator('[data-action="open-repository-file-menu"]').last().click({button:'right'});
  const narrowBounds=(await repositoryMenu.boundingBox())!;
  expect(narrowBounds.x+narrowBounds.width).toBeLessThanOrEqual(752);
  expect(narrowBounds.y+narrowBounds.height).toBeLessThanOrEqual(632);
  await repository.screenshot({path:'/private/tmp/hs2-jgfm53-repository-file-menu-narrow.png'});

  await page.setViewportSize({width:1280,height:900});
  await page.goto('/ux-demo?component=change-evidence-dialog');
  const evidence=page.locator('[data-component="change-evidence-dialog"]');
  await evidence.locator('[data-action="open-ticket-file-menu"]').click();
  await expect(evidence.getByRole('menuitem',{name:'Show Diff'})).toBeEnabled();
  await evidence.screenshot({path:'/private/tmp/hs2-jgfm53-change-evidence-file-menu-wide.png'});
});

test('represents every server-build details state with shared dialog geometry',async({page})=>{
  await page.setViewportSize({width:1280,height:900});await page.goto('/ux-demo?component=connection-details-dialog');const dialog=page.locator('[data-component="connection-details-dialog"]');await expect(dialog).toBeVisible();await expect(dialog).toHaveAttribute('data-embedded','true');await expect(dialog.locator('[data-component="dialog-header"]')).toBeVisible();await expect(dialog.locator('[data-component="value-table"]')).toBeVisible();await expect(dialog.getByRole('button',{name:'Close'})).toHaveCount(0);
  await page.locator('[data-action="toggle-settings"]').click();const scenario=page.getByRole('complementary',{name:'ConnectionDetailsDialog settings'}).locator('wa-select[name="scenario"]');for(const [value,kind,copy] of [['source-stale','compatible','cargo build -p hotsheet-server'],['revision-mismatch','compatible','detached build is intentional'],['server-too-old-safe','server_too_old','quiescence-safe restart'],['server-too-old-manual','server_too_old','Automatic restart is unavailable'],['client-too-old','client_too_old','Update this Hot Sheet client'],['unknown','unknown','inspect the server log']] as const){await scenario.evaluate((node:HTMLElement&{value:string},next)=>{node.value=next;node.dispatchEvent(new Event('change',{bubbles:true}))},value);await expect(dialog).toHaveAttribute('data-kind',kind);await expect(dialog).toContainText(copy)}await page.getByRole('complementary',{name:'ConnectionDetailsDialog settings'}).getByRole('button',{name:'Reset'}).click();await expect(scenario).toHaveJSProperty('value','source-stale');await page.locator('[data-action="toggle-settings"]').click();await dialog.screenshot({path:'/private/tmp/hs2-xrs9s4-server-details-demo-wide.png'});await page.setViewportSize({width:390,height:844});await dialog.screenshot({path:'/private/tmp/hs2-xrs9s4-server-details-demo-narrow.png'});
});

test('represents the production terminal dashboard and its shared context menu in the UX catalog',async({page})=>{
  await page.setViewportSize({width:1280,height:900});await page.goto('/ux-demo?component=terminal-dashboard');const dashboard=page.getByRole('region',{name:'Terminal dashboard'});await expect(dashboard).toBeVisible();await expect(dashboard).toHaveAttribute('data-basis','high');await expect(dashboard).toHaveAttribute('data-fit','3');await expect(dashboard.locator('[data-fixed-aspect-terminal-card="preview"]')).toHaveCount(2);const menu=dashboard.getByRole('menu');await expect(menu).toHaveCount(0);const first=dashboard.locator('[data-component="terminal-tile"]').first();await first.getByRole('button',{name:/More actions/}).click();await expect(menu.getByRole('menuitem')).toHaveCount(2);await expect(menu.getByText('Open')).toBeVisible();await expect(menu.getByText('Hide Terminal')).toBeVisible();await page.keyboard.press('Escape');await expect(menu).toHaveCount(0);await first.click({button:'right'});await expect(menu).toBeVisible();await page.screenshot({path:'/private/tmp/hs2-fw96jm-terminal-dashboard-menu-on-demand.png',fullPage:true});
});

test('represents aggregate and per-project terminal operations in the UX catalog',async({page})=>{
  await page.setViewportSize({width:1280,height:900});await page.goto('/ux-demo?component=terminal-operations-sidebar');const sidebar=page.getByRole('complementary',{name:'Terminal operations sidebar'});await expect(sidebar).toBeVisible();await expect(sidebar.locator('[data-component="menu-header"]')).toHaveCount(3);await expect(sidebar.locator('[data-component="project-summary"]')).toHaveCount(3);await expect(sidebar.getByText('All projects',{exact:true})).toBeVisible();await expect(sidebar.getByText('Hot Sheet',{exact:true})).toBeVisible();await expect(sidebar.getByText('Documentation',{exact:true})).toBeVisible();const aggregate=sidebar.locator('[data-project-id="all"] [data-component="project-summary"]');await expect(aggregate).toContainText('8 completed today');await expect(aggregate).toHaveAttribute('data-chart-tone','success');await expect(sidebar.locator('[data-project-id="hotsheet"] [data-component="project-summary"]')).toHaveAttribute('data-chart-tone','brand');await page.screenshot({path:'/private/tmp/hs2-mh8qn2-terminal-operations-green-aggregate.png',fullPage:true});
});

test('represents the compact terminal ticket rail in the UX catalog',async({page})=>{
  await page.setViewportSize({width:1280,height:900});await page.goto('/ux-demo?component=terminal-ticket-rail');const rail=page.locator('[data-component="terminal-ticket-rail"]'),project=rail.locator('wa-select[name="terminal-rail-project"]'),view=rail.locator('wa-select[name="terminal-rail-view"]');await expect(rail).toBeVisible();await expect(project).toHaveAttribute('value','demo');await expect(view).toHaveAttribute('value','all');await expect(rail.getByRole('button',{name:'Hide ticket rail'})).toBeVisible();await expect(rail.locator('[data-component="ticket-list-row"]')).toHaveCount(7);await expect(rail.locator('[data-component="content-transition"]')).toHaveAttribute('data-transition-style','push');const geometry=await rail.evaluate(node=>{const modeElement=node.querySelector<HTMLElement>('.view-mode-switcher')!,mode=modeElement.getBoundingClientRect(),sort=node.querySelector('.workspace-header__sort-group')!.getBoundingClientRect(),search=node.querySelector('.workspace-header__search-group')!.getBoundingClientRect(),utility=node.querySelector('.workspace-header__utility-group')!.getBoundingClientRect(),project=node.querySelector('wa-select[name="terminal-rail-project"]')!.getBoundingClientRect();return{mode:{top:mode.top,bottom:mode.bottom,width:mode.width,radius:getComputedStyle(modeElement).borderRadius},sortTop:sort.top,searchTop:search.top,searchLeft:search.left,utilityTop:utility.top,utilityRight:utility.right,projectWidth:project.width,railWidth:node.getBoundingClientRect().width}});expect(geometry.mode.bottom).toBeLessThanOrEqual(geometry.sortTop);expect(geometry.searchTop).toBeCloseTo(geometry.sortTop,0);expect(geometry.utilityTop).toBeCloseTo(geometry.sortTop,0);expect(geometry.searchLeft).toBeGreaterThanOrEqual(geometry.utilityRight);expect(geometry.mode.radius).not.toBe('9999px');expect(geometry.mode.width).toBeGreaterThan(geometry.railWidth*.8);expect(geometry.projectWidth).toBeLessThan(geometry.railWidth*.8);await page.screenshot({path:'/private/tmp/hs2-egekzg-terminal-ticket-rail-ux-demo-wide.png',fullPage:true});
});

test('catalogs both FixedAspectTerminalCard variants and their dashboard relationship',async({page})=>{
  await page.setViewportSize({width:1728,height:971});await page.goto('/ux-demo?component=fixed-aspect-terminal-card');const stage=page.getByRole('region',{name:'Fixed aspect terminal card variants'}),preview=stage.locator('[data-fixed-aspect-terminal-card="preview"]'),magnified=stage.locator('[data-fixed-aspect-terminal-card="magnified"]'),previewViewport=preview.locator('[data-display-mode="scaled-preview"]'),magnifiedViewport=magnified.locator('[data-display-mode="interactive"]');await expect(preview).toBeVisible();await expect(magnified).toBeVisible();for(const viewport of [previewViewport,magnifiedViewport]){await expect(viewport).toHaveAttribute('data-connection','connected');await expect(viewport).toHaveAttribute('data-grid-size','80x24');await expect(viewport).toHaveAttribute('data-renderer','dom');const rows=viewport.locator('.xterm-rows > div');await expect(rows).toHaveCount(24);await expect(rows.first()).toContainText('GNU nano 8.4');await expect(rows.nth(1)).toContainText('File: src/main.tsx');await expect(rows.nth(20)).toContainText('export { app };');await expect(rows.last()).toContainText('^X Exit');for(const bar of [rows.first(),rows.nth(1),rows.nth(21),rows.nth(22),rows.last()])expect(await bar.evaluate(node=>node.textContent.length)).toBe(80)}await expect(preview).toHaveCSS('border-width','0px');await expect(magnified).toHaveCSS('border-width','0px');const sizing=await stage.evaluate(element=>{const preview=element.querySelector<HTMLElement>('[data-fixed-aspect-terminal-card="preview"]')!.getBoundingClientRect(),magnified=element.querySelector<HTMLElement>('[data-fixed-aspect-terminal-card="magnified"]')!.getBoundingClientRect(),row=element.querySelector<HTMLElement>('[data-fixed-aspect-terminal-card="preview"] .xterm-rows > div')!;return{previewWidth:preview.width,magnifiedWidth:magnified.width,font:getComputedStyle(row).fontFamily}});expect(sizing.previewWidth).toBeLessThanOrEqual(352);expect(sizing.magnifiedWidth).toBeGreaterThan(sizing.previewWidth);expect(sizing.font).toContain('ui-monospace');const relationships=page.locator('.demo-relationships');await expect(relationships.locator('.select__group')).toHaveAttribute('aria-label','Used by');await expect(relationships.locator('wa-option',{hasText:'TerminalDashboard'})).toHaveCount(1);await relationships.click();await expect(relationships).toHaveJSProperty('open',true);await page.waitForTimeout(1_200);await expect(relationships).toHaveJSProperty('open',true);await page.keyboard.press('Escape');await page.screenshot({path:'/private/tmp/hs2-g4g95r-nano-fills-terminal.png',fullPage:true});await page.setViewportSize({width:390,height:844});await expect(preview).toBeVisible();await expect(magnified).toBeVisible();await expect.poll(()=>stage.evaluate(node=>node.scrollWidth<=node.clientWidth)).toBe(true);await page.screenshot({path:'/private/tmp/hs2-g4g95r-nano-fills-terminal-narrow.png',fullPage:true});
});

test('represents interactive terminal visibility groups in the UX catalog',async({page})=>{
  await page.setViewportSize({width:1280,height:900});await page.goto('/ux-demo?component=terminal-visibility-dialog');const show=page.getByRole('button',{name:'Show / Hide Terminals'}),dialog=page.locator('[data-terminal-visibility-dialog]');await expect(show).toBeVisible();await expect(dialog).toHaveJSProperty('open',false);await show.click();await expect(dialog).toHaveJSProperty('open',true);await expect(dialog.locator('.terminal-visibility-dialog__toolbar')).toHaveCSS('background-color','rgba(0, 0, 0, 0)');await expect(dialog.getByRole('tab',{name:'Focus'})).toHaveAttribute('aria-selected','true');await dialog.getByRole('button',{name:'Add visibility group'}).click();const nameDialog=page.locator('[data-terminal-visibility-name-dialog]'),name=nameDialog.getByRole('textbox',{name:'Group name'});await expect(nameDialog).toHaveJSProperty('open',true);await expect(name).toBeFocused();await name.fill('Review');await nameDialog.getByRole('button',{name:'Add'}).click();await expect(nameDialog).toHaveJSProperty('open',false);const review=dialog.getByRole('tab',{name:'Review'});await expect(review).toHaveAttribute('aria-selected','true');await dialog.getByRole('button',{name:/Hide Development/}).click();await expect(dialog.getByRole('button',{name:/Show Development/})).toBeVisible();await page.screenshot({path:'/private/tmp/hs2-z0m2vv-visibility-demo-open-wide.png',fullPage:true});await page.keyboard.press('Escape');await expect(dialog).toHaveJSProperty('open',false);await expect(show).toBeVisible();await show.click();await expect(dialog).toHaveJSProperty('open',true);await page.setViewportSize({width:390,height:844});await expect(dialog.locator('.terminal-visibility-dialog__toolbar')).toHaveCSS('background-color','rgba(0, 0, 0, 0)');await page.screenshot({path:'/private/tmp/hs2-z0m2vv-visibility-demo-open-narrow.png',fullPage:true});await page.setViewportSize({width:1280,height:900});await review.click({button:'right'});const menu=dialog.getByRole('menu',{name:'Visibility group actions'});await expect(menu.getByText('Rename…')).toBeVisible();await expect(menu.getByText('Delete')).toBeVisible();
});

test('captures, reviews, cancels, and submits dev-review feedback', async ({ page }) => {
  let submitted: Record<string, unknown> | undefined;
  await page.route('**/__hotsheet/dev-review/tickets', async route => {
    submitted = route.request().postDataJSON();
    await route.fulfill({ status: 201, contentType: 'application/json', body: JSON.stringify({ slug: 'HS2-REVIEW' }) });
  });
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto('/ux-demo?component=ticket-row&dev-review=1');
  const captureTarget = page.locator('.demo-master [data-item-id="ticket-row"]');
  await captureTarget.scrollIntoViewIfNeeded();
  await captureTarget.evaluate(node => { (node as HTMLElement).style.color = 'oklab(55% 0.1 0.1)'; });
  await page.locator('.demo-detail__header').evaluate(node => { (node as HTMLElement).style.boxShadow = '0 0 2px oklab(55% 0.1 0.1)'; });
  expect(await captureTarget.evaluate(node => getComputedStyle(node).color)).toMatch(/^(?:oklab|rgb)/);
  const markerBox = (await captureTarget.boundingBox())!;
  const tool = page.locator('.hs-dev-review');
  await expect(tool.getByRole('button', { name: 'Feedback' })).toBeVisible();
  await tool.getByRole('button', { name: 'Feedback' }).click();
  await expect(tool.getByRole('button', { name: 'New Ticket' })).toBeVisible();
  const hint = tool.locator('.hs-dev-review__hint');
  await expect(hint).toBeVisible();
  await page.waitForTimeout(3200);
  await expect(hint).toHaveClass(/hs-dev-review__hint--hidden/);
  await page.keyboard.down('Alt');
  await expect(page.locator('html')).toHaveClass(/hs-dev-review--crosshair/);
  await expect(page.locator('body')).toHaveCSS('cursor', 'crosshair');
  await page.mouse.move(markerBox.x, markerBox.y); await page.mouse.down(); await page.mouse.move(markerBox.x + markerBox.width, markerBox.y + markerBox.height); await page.mouse.up();
  await page.keyboard.up('Alt');
  await expect(page.locator('html')).not.toHaveClass(/hs-dev-review--crosshair/);
  const selection = tool.locator('.hs-dev-review__rect');
  await expect(selection).toHaveCount(1);
  await expect(selection.locator('.hs-dev-review__handle')).toHaveCount(8);
  const beforeScroll = await selection.boundingBox();
  const scroller = page.locator('.demo-master');
  const initialScroll = await scroller.evaluate(node => node.scrollTop);
  await scroller.evaluate(node => { node.scrollBy(0, -80); });
  await expect.poll(() => scroller.evaluate(node => node.scrollTop)).toBeLessThan(initialScroll);
  const scrollDelta = await scroller.evaluate((node, start) => node.scrollTop - start, initialScroll);
  await expect.poll(async () => (await selection.boundingBox())!.y).toBeCloseTo(beforeScroll!.y - scrollDelta, 0);
  const stableSelectionNode = await selection.elementHandle();
  const before = await selection.boundingBox();
  const resize = selection.getByRole('button', { name: /Resize capture 1 from se/ });
  const handle = await resize.boundingBox();
  await page.mouse.move(handle!.x + 4, handle!.y + 4); await page.mouse.down(); await page.mouse.move(handle!.x + 54, handle!.y + 34); await page.mouse.up();
  const after = await selection.boundingBox();
  expect(after!.width).toBeGreaterThan(before!.width);
  expect(after!.height).toBeGreaterThan(before!.height);
  expect(await stableSelectionNode.evaluate(node => node.isConnected)).toBe(true);
  const corner = await resize.boundingBox();
  expect(Math.abs(corner!.width - corner!.height)).toBeLessThanOrEqual(1);
  const eastResize = selection.getByRole('button', { name: /Resize capture 1 from e$/ });
  await expect(eastResize).toHaveCSS('cursor', 'ew-resize');
  const eastHandle = await eastResize.boundingBox();
  const beforeEast = await selection.boundingBox();
  await page.mouse.move(eastHandle!.x + 4, eastHandle!.y + 10); await page.mouse.down(); await page.mouse.move(eastHandle!.x + 34, eastHandle!.y + 10); await page.mouse.up();
  const afterEast = await selection.boundingBox();
  expect(afterEast!.width).toBeGreaterThan(beforeEast!.width);
  expect(Math.abs(afterEast!.height - beforeEast!.height)).toBeLessThanOrEqual(1);
  await page.keyboard.down('Alt');
  await page.mouse.move(720, 300); await page.mouse.down(); await page.mouse.move(920, 500); await page.mouse.up();
  await page.keyboard.up('Alt');
  await expect(tool.locator('.hs-dev-review__rect')).toHaveCount(2);
  const removable = tool.locator('.hs-dev-review__rect').last();
  const removableBox = await removable.boundingBox();
  await page.keyboard.down('Alt'); await page.keyboard.down('Shift');
  await page.mouse.move(removableBox!.x + 30, removableBox!.y + 30);
  await expect(removable).toHaveCSS('cursor', 'not-allowed');
  await page.mouse.click(removableBox!.x + 30, removableBox!.y + 30);
  await page.keyboard.up('Shift'); await page.keyboard.up('Alt');
  await expect(tool.locator('.hs-dev-review__rect')).toHaveCount(1);
  await page.keyboard.down('Alt');
  await page.mouse.move(720, 300); await page.mouse.down(); await page.mouse.move(920, 500); await page.mouse.up();
  await page.keyboard.up('Alt');
  await expect(tool.locator('.hs-dev-review__rect')).toHaveCount(2);
  await page.waitForTimeout(400);
  await tool.getByRole('button', { name: 'New Ticket' }).click();
  const dialog = page.getByRole('dialog', { name: 'New Hot Sheet ticket' });
  await expect(dialog).toBeVisible();
  await expect(dialog.locator('.dialog-header')).toHaveCSS('border-bottom-width', '0px');
  await expect(dialog.locator('footer')).toHaveCSS('border-top-width', '0px');
  expect(await dialog.evaluate(node => ({
    border: getComputedStyle(node).borderColor,
    divider: getComputedStyle(document.documentElement).getPropertyValue('--hs-shell-divider').trim(),
    surface: getComputedStyle(node).backgroundColor,
  }))).toEqual({ border: 'rgb(209, 209, 214)', divider: '#d1d1d6', surface: 'rgb(255, 255, 255)' });
  await expect(dialog.getByRole('button', { name: 'Review captured region 1' })).toBeVisible();
  await expect(dialog.getByRole('button', { name: 'Review captured region 2' })).toBeVisible();
  await expect(dialog.getByRole('img', { name: 'Captured region 1 preview' })).toHaveAttribute('src', /^data:image\/png;base64,/);
  await page.screenshot({ path: '/private/tmp/hs2-66m88k-dev-review-theme-wide.png', fullPage: true });
  await page.setViewportSize({ width: 760, height: 900 });
  await expect(dialog).toBeVisible();
  await page.screenshot({ path: '/private/tmp/hs2-66m88k-dev-review-theme-narrow.png', fullPage: true });
  await page.setViewportSize({ width: 1280, height: 900 });
  const capturedPixels = await dialog.getByRole('img', { name: 'Captured region 1 preview' }).evaluate(async image => {
    if (!(image as HTMLImageElement).complete) await new Promise(resolve => { image.addEventListener('load', resolve, { once: true }); });
    const canvas = document.createElement('canvas'); canvas.width = (image as HTMLImageElement).naturalWidth; canvas.height = (image as HTMLImageElement).naturalHeight; const context = canvas.getContext('2d')!; context.drawImage(image as HTMLImageElement, 0, 0); const pixels = context.getImageData(0, 0, canvas.width, canvas.height).data; let green = 0; for (let index = 0; index < pixels.length; index += 4) if (pixels[index] === 12 && pixels[index + 1] === 200 && pixels[index + 2] === 34) green += 1; return { center: [...context.getImageData(Math.floor(canvas.width / 2), Math.floor(canvas.height / 2), 1, 1).data], green, width: canvas.width, height: canvas.height };
  });
  expect(capturedPixels.center.slice(0, 3), JSON.stringify(capturedPixels)).toEqual([214, 236, 255]);
  await dialog.getByRole('button', { name: 'Review captured region 2' }).click();
  await expect(dialog.getByRole('img', { name: 'Captured region 2 preview' })).toBeVisible();
  const attachmentInput = dialog.getByLabel('Add attachments');
  await attachmentInput.setInputFiles({ name: 'notes.txt', mimeType: 'text/plain', buffer: Buffer.from('review notes') });
  await expect(dialog.getByText('notes.txt')).toBeVisible();
  await dialog.getByRole('button', { name: 'Remove attachment notes.txt' }).click();
  await expect(dialog.getByText('notes.txt')).toHaveCount(0);
  const dropzone = dialog.locator('.hs-dev-review__dropzone');
  await dropzone.evaluate(node => {
    const transfer = new DataTransfer(); transfer.items.add(new File(['log'], 'debug.log', { type: 'text/plain' }));
    node.dispatchEvent(new DragEvent('drop', { bubbles: true, cancelable: true, dataTransfer: transfer }));
  });
  await expect(dialog.getByText('debug.log')).toBeVisible();
  await dialog.getByRole('button', { name: 'Remove captured region 2' }).click();
  await expect(dialog.getByRole('button', { name: 'Review captured region 2' })).toHaveCount(0);
  await expect(tool.locator('.hs-dev-review__rect')).toHaveCount(1);
  await expect(dialog.getByRole('button', { name: 'Cancel' })).toHaveCount(1);
  await dialog.getByRole('button', { name: 'Cancel' }).click();
  await expect(dialog).toHaveCount(0);
  await expect(tool.getByRole('button', { name: 'New Ticket' })).toBeVisible();
  page.once('dialog', dialog => void dialog.dismiss());
  await tool.getByRole('button', { name: 'Feedback' }).click();
  await expect(tool.locator('.hs-dev-review__rect')).toHaveCount(1);
  page.once('dialog', dialog => void dialog.accept());
  await tool.getByRole('button', { name: 'Feedback' }).click();
  await expect(tool.getByRole('button', { name: 'New Ticket' })).toHaveCount(0);
  await expect(tool.locator('.hs-dev-review__rect')).toHaveCount(0);
  await tool.getByRole('button', { name: 'Feedback' }).click();
  await page.keyboard.down('Alt');
  await page.mouse.move(500, 240); await page.mouse.down(); await page.mouse.move(900, 480); await page.mouse.up();
  await page.keyboard.up('Alt');
  await tool.getByRole('button', { name: 'New Ticket' }).click();
  const reopened = page.getByRole('dialog', { name: 'New Hot Sheet ticket' });
  await reopened.getByLabel('Add attachments').setInputFiles({ name: 'context.txt', mimeType: 'text/plain', buffer: Buffer.from('context') });
  await reopened.getByRole('textbox', { name: 'Feedback notes' }).fill('The selected row spacing is inconsistent.');
  await reopened.getByRole('button', { name: 'Create Ticket' }).click();
  await expect(reopened.getByRole('status')).toContainText('HS2-REVIEW created');
  await expect.poll(() => submitted).toBeTruthy();
  expect(submitted).toMatchObject({ notes: 'The selected row spacing is inconsistent.', captures: [{ filename: expect.stringMatching(/^ux-feedback-\d+\.png$/) }], attachments: [{ filename: 'context.txt', mimeType: 'text/plain' }] });
  expect((submitted!.captures as Array<{ dataUrl: string }>)[0].dataUrl).toMatch(/^data:image\/png;base64,/);
  await expect(tool.getByRole('button', { name: 'New Ticket' })).toHaveCount(0);
});

test('keeps feedback rectangle input within its frame budget in the UX demo', async ({ page }, testInfo) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto('/ux-demo?component=ticket-row&dev-review=1');
  const measurement = await measureFeedbackRectangle(page, { x: 460, y: 260 }, { x: 780, y: 460 });
  await testInfo.attach('feedback-performance.json', { body: JSON.stringify(measurement, null, 2), contentType: 'application/json' });
  expectResponsiveFeedbackRectangle(measurement);
  await page.screenshot({ path: '/private/tmp/hs2-6ppvjc-ux-demo-wide.png', fullPage: true });
  await page.setViewportSize({ width: 390, height: 844 });
  await page.screenshot({ path: '/private/tmp/hs2-6ppvjc-ux-demo-narrow.png', fullPage: true });
});

test('round-trips StatusBadge controls through reset and a post-reset edit', async ({ page }) => {
  await page.goto('/ux-demo?component=status-badge');
  const badge = page.locator('[data-component="status-badge"]');
  await expect(badge).toContainText('Started');
  await expect(badge.locator('[data-lucide="clock"]')).toHaveAttribute('aria-hidden', 'true');
  await page.locator('[data-action="toggle-settings"]').click();
  const inspector = page.getByRole('complementary', { name: 'StatusBadge settings' });
  const status = inspector.locator('wa-select[name="status"]');
  const appearance = inspector.locator('wa-select[name="appearance"]');
  const icon = inspector.locator('wa-checkbox[name="show-icon"]');
  const compact = inspector.locator('wa-checkbox[name="compact"]');
  await status.evaluate((node: HTMLElement & { value: string }) => { node.value = 'verified'; node.dispatchEvent(new Event('change', { bubbles: true })); });
  await expect(badge).toContainText('Verified');
  await expect(badge.locator('[data-lucide="badge-check"]')).toHaveCount(1);
  await appearance.evaluate((node: HTMLElement & { value: string }) => { node.value = 'plain'; node.dispatchEvent(new Event('change', { bubbles: true })); });
  await expect(badge).toHaveAttribute('data-appearance', 'plain');
  await expect(badge).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
  await compact.click();
  await expect(badge).toHaveClass(/status-badge--compact/);
  await icon.click();
  await expect(badge.locator('.status-badge__icon')).toHaveCount(0);
  await inspector.getByRole('button', { name: 'Reset' }).click();
  await expect(status).toHaveJSProperty('value', 'started');
  await expect(appearance).toHaveJSProperty('value', 'filled');
  await expect(icon).toHaveJSProperty('checked', true);
  await expect(compact).toHaveJSProperty('checked', false);
  await expect(badge).toContainText('Started');
  await expect(badge).toHaveAttribute('data-appearance', 'filled');
  await expect(badge).not.toHaveClass(/status-badge--compact/);
  await expect(badge.locator('[data-lucide="clock"]')).toHaveCount(1);
  await expect(badge.locator('.status-badge__icon')).toHaveCount(1);
  await status.evaluate((node: HTMLElement & { value: string }) => { node.value = 'completed'; node.dispatchEvent(new Event('change', { bubbles: true })); });
  await expect(badge).toContainText('Completed');
  await expect(badge.locator('[data-lucide="circle-check"]')).toHaveCount(1);
});

test('demonstrates the production Not Working dialog and pending evidence picker',async({page})=>{
  await page.goto('/ux-demo?component=not-working-dialog');await page.getByRole('button',{name:'Open Not Working dialog'}).click();const dialog=page.getByRole('dialog',{name:'Not Working — HS2-DEMO'});
  // wa-dialog projects its content into a shadow-DOM native <dialog> in the top layer, so the
  // host element itself has zero height — asserting toBeVisible() on the host is wrong (the real
  // app's tests assert content/focus instead). Assert the dialog opened, then that its content shows.
  await expect(dialog).toHaveAttribute('open', '');await expect(dialog.getByText('diagnostic screenshot with a deliberately long filename.png')).toBeVisible();await dialog.getByRole('textbox',{name:'What’s wrong?'}).fill('The verification failed.');await dialog.getByLabel('Browse evidence attachments').setInputFiles({name:'new-proof.txt',mimeType:'text/plain',buffer:Buffer.from('proof')});await expect(dialog.getByText('new-proof.txt')).toBeVisible();await dialog.getByRole('button',{name:'Remove new-proof.txt'}).click();await expect(dialog.getByText('new-proof.txt')).toHaveCount(0);await dialog.getByRole('button',{name:'Report Not Working'}).click();await expect(page.getByText('Ticket returned to Not Started and added to Up Next.')).toBeVisible();
});

test('round-trips every TicketRow setting and selection action', async ({ page }) => {
  await page.goto('/ux-demo?component=ticket-row');
  const row = page.locator('[data-component="ticket-list-row"]');
  await expect(row).toContainText('Build the first client ticket list');
  await page.locator('[data-action="toggle-settings"]').click();
  const inspector = page.getByRole('complementary', { name: 'TicketRow settings' });
  const title = inspector.getByRole('textbox', { name: 'Title' });
  const category = inspector.getByRole('textbox', { name: 'Category' });
  const tags = inspector.getByRole('textbox', { name: 'Tags (comma separated)' });
  const agent = inspector.getByRole('textbox', { name: 'Active agent' });
  const updated = inspector.getByRole('textbox', { name: 'Updated label' });
  const status = inspector.locator('wa-select[name="status"]');
  const priority = inspector.locator('wa-select[name="priority"]');
  const categoryIcon = inspector.locator('wa-select[name="category-icon"]');
  const categoryColor = inspector.locator('wa-select[name="category-color"]');
  const upNext = inspector.locator('wa-checkbox[name="up-next"]');
  const blocked = inspector.locator('wa-checkbox[name="blocked"]');
  const needsReview = inspector.locator('wa-checkbox[name="needs-review"]');
  const selected = inspector.locator('wa-checkbox[name="selected"]');
  const busy = inspector.locator('wa-checkbox[name="busy"]');
  await title.fill('Fix selection synchronization');
  await category.fill('bug');
  await tags.fill('client, regression');
  await agent.fill('Codex');
  await updated.fill('Now');
  await status.evaluate((node: HTMLElement & { value: string }) => { node.value = 'verified'; node.dispatchEvent(new Event('change', { bubbles: true })); });
  await priority.evaluate((node: HTMLElement & { value: string }) => { node.value = 'urgent'; node.dispatchEvent(new Event('change', { bubbles: true })); });
  await categoryIcon.evaluate((node: HTMLElement & { value: string }) => { node.value = 'bug'; node.dispatchEvent(new Event('change', { bubbles: true })); });
  await categoryColor.evaluate((node: HTMLElement & { value: string }) => { node.value = '#ef4444'; node.dispatchEvent(new Event('change', { bubbles: true })); });
  await upNext.click();
  await blocked.click();
  await needsReview.click();
  await selected.click();
  await busy.click();
  const feedbackNeeded = inspector.locator('wa-checkbox[name="feedback-needed"]');
  await expect(row.locator('.ticket-list-row__feedback')).toContainText('Needs review');
  await feedbackNeeded.click();
  await expect(row.locator('.ticket-list-row__feedback')).toContainText('Needs review');
  await expect(row.locator('.ticket-list-row__feedback [data-lucide="circle-alert"]')).toHaveCount(1);
  await needsReview.click();
  await expect(row.locator('.ticket-list-row__feedback')).toContainText('Needs review');
  await feedbackNeeded.click();
  await expect(row.locator('.ticket-list-row__feedback')).toHaveCount(0);
  await feedbackNeeded.click();
  await expect(row).toContainText('Fix selection synchronization');
  await expect(row).toContainText('Verified');
  await expect(row.locator('[data-component="blocked-badge"]')).toHaveText('Blocked');
  // Rows use the colored (filled) status variant like the inspector (HS2-Y3H2Z5).
  await expect(row.locator('[data-component="status-badge"]')).toHaveAttribute('data-appearance', 'filled');
  await expect(row.locator('[data-component="status-badge"]')).toHaveCSS('background-color', 'color(srgb 0.84 0.925333 1)');
  await expect(row.locator('[data-lucide="bug"]')).toHaveCount(1);
  await expect(row.locator('.ticket-list-row__category')).toHaveCSS('color', 'rgb(239, 68, 68)');
  await categoryColor.evaluate((node: HTMLElement & { value: string }) => { node.value = '#e5e7eb'; node.dispatchEvent(new Event('change', { bubbles: true })); });
  await expect(row.locator('.ticket-list-row__category')).toHaveCSS('color', 'rgb(156, 163, 175)');
  await categoryIcon.evaluate((node: HTMLElement & { value: string }) => { node.value = ''; node.dispatchEvent(new Event('change', { bubbles: true })); });
  await expect(row.locator('.ticket-list-row__category--label')).toHaveText('BUG');
  await expect(row.locator('.ticket-list-row__category--label')).toHaveCSS('color', 'rgb(156, 163, 175)');
  await expect(row.locator('.ticket-list-row__category--label')).toHaveCSS('width', '32px');
  await expect(row).toContainText('regression');
  await expect(row.locator('[data-lucide="star"]')).toHaveCount(0);
  await expect(row.locator('[data-action="toggle-row-up-next"]')).toHaveCount(0);
  await expect(row).toContainText('Codex');
  await expect(row).toContainText('Now');
  await expect(row).toHaveAttribute('data-selected', 'true');
  await expect(row.locator('[data-lucide="chevrons-up"]')).toHaveCount(1);
  await expect(row.locator('.ticket-list-row__priority')).toHaveCSS('color', 'rgb(255, 56, 60)');
  await expect(row.locator('.ticket-list-row__indicator')).toHaveClass(/needs-review/);
  await row.click();
  await expect(selected).toHaveJSProperty('checked', false);
  await expect(row).toHaveAttribute('data-selected', 'false');
  await expect(page.getByText('Ticket deselected')).toBeVisible();
  await inspector.getByRole('button', { name: 'Reset' }).click();
  await expect(title).toHaveJSProperty('value', 'Build the first client ticket list');
  await expect(status).toHaveJSProperty('value', 'started');
  await expect(priority).toHaveJSProperty('value', 'high');
  await expect(category).toHaveJSProperty('value', 'feature');
  await expect(tags).toHaveJSProperty('value', 'client, ux');
  await expect(agent).toHaveJSProperty('value', 'Claude');
  await expect(updated).toHaveJSProperty('value', '1h ago');
  await expect(upNext).toHaveJSProperty('checked', true);
  await expect(blocked).toHaveJSProperty('checked', false);
  await expect(needsReview).toHaveJSProperty('checked', false);
  await expect(categoryIcon).toHaveJSProperty('value', 'sparkles');
  await expect(categoryColor).toHaveJSProperty('value', '#3b82f6');
  await expect(selected).toHaveJSProperty('checked', false);
  await expect(busy).toHaveJSProperty('checked', true);
  await expect(row).toContainText('Build the first client ticket list');
  await expect(row).toContainText('Started');
  await expect(row.locator('[data-action="toggle-row-up-next"]')).toHaveClass(/active/);
  await expect(row.locator('.ticket-list-row__category')).toHaveCSS('width', '32px');
  await expect(row.locator('.ticket-list-row__indicator')).toHaveClass(/up-next/);
  const upNextRailColor = await row.locator('.ticket-list-row__indicator').evaluate(element => getComputedStyle(element).backgroundColor);
  await expect(row.locator('[data-action="toggle-row-up-next"]')).toHaveCSS('color', upNextRailColor);
  await expect(row).toContainText('Claude');
  await expect(row).toContainText('1h ago');
  await expect(page.getByText('No actions yet')).toBeVisible();
  await title.fill('Post-reset edit works');
  await expect(row).toContainText('Post-reset edit works');
  await row.focus();
  await page.keyboard.press('Enter');
  await expect(row).toHaveAttribute('data-selected', 'true');
  await expect(selected).toHaveJSProperty('checked', true);

  const star = row.locator('[data-action="toggle-row-up-next"]');
  const starBox = await star.boundingBox();
  expect(starBox).not.toBeNull();
  await star.click();
  await expect(upNext).toHaveJSProperty('checked', false);
  await expect(row).toHaveAttribute('data-selected', 'true');
  await expect(star).not.toHaveClass(/active/);
  await star.focus();
  await page.keyboard.press('Enter');
  await expect(upNext).toHaveJSProperty('checked', true);
  await expect(star).toHaveClass(/active/);

  await row.click({ button: 'right' });
  const menu = page.getByRole('menu', { name: 'Ticket actions' });
  await expect(menu).toBeVisible();
  const menuItems = menu.locator('wa-dropdown-item');
  await expect(menu.locator(':scope > wa-dropdown > wa-dropdown-item')).toHaveCount(11);
  await expect(menuItems).toHaveCount(27);
  expect(await menuItems.evaluateAll(items => items.every(item => item.querySelector('[data-lucide]') !== null))).toBe(true);
  await expect(row).toHaveAttribute('data-selected', 'true');
  await menu.getByText('Toggle Up Next', { exact: true }).click();
  await expect(star).not.toHaveClass(/active/);
  await expect(upNext).toHaveJSProperty('checked', false);
  await expect(page.getByText('Toggle Up Next selected')).toBeVisible();

  await title.fill('A deliberately long ticket title that must wrap across no more than two lines while the AI working indicator remains entirely visible');
  await priority.evaluate((node: HTMLElement & { value: string }) => { node.value = 'default'; node.dispatchEvent(new Event('change', { bubbles: true })); });
  await expect(row.locator('[data-lucide="minus"]')).toHaveCount(1);
  await tags.fill('client, regression, server, ux');
  await row.evaluate((node: HTMLElement) => { node.style.width = '320px'; });
  const [rowBox, timeBox, slugBox, identityBox, titleLineBoxes] = await Promise.all([
    row.boundingBox(),
    row.locator('.ticket-list-row__updated').boundingBox(),
    row.locator('.ticket-list-row__slug').boundingBox(),
    row.locator('.ticket-list-row__identity').boundingBox(),
    row.locator('.ticket-list-row__identity strong').evaluate(node => [...node.getClientRects()].map(rect => ({ x: rect.x, width: rect.width }))),
  ]);
  expect(rowBox).not.toBeNull(); expect(timeBox).not.toBeNull(); expect(slugBox).not.toBeNull(); expect(identityBox).not.toBeNull();
  expect(timeBox!.x + timeBox!.width).toBeLessThanOrEqual(rowBox!.x + rowBox!.width);
  expect(Math.abs(timeBox!.y + timeBox!.height - (slugBox!.y + slugBox!.height))).toBeLessThanOrEqual(3);
  expect(identityBox!.height).toBeLessThanOrEqual(43);
  await expect(row.locator('.ticket-list-row__identity')).toHaveCSS('display', 'block');
  await expect(row.locator('.ticket-list-row__updated')).toHaveCSS('float', 'right');
  await expect(row.locator('.ticket-list-row__slug')).toHaveCSS('display', 'inline-block');
  await expect(row.locator('.ticket-list-row__priority')).toHaveCSS('display', 'inline');
  expect(await row.locator('.ticket-list-row__identity > *').evaluateAll(elements => elements.map(element => element.className || element.tagName.toLowerCase()))).toEqual([
    'ticket-list-row__updated', 'ticket-list-row__slug', 'ticket-list-row__priority', 'strong',
  ]);
  expect(titleLineBoxes.length).toBeGreaterThan(1);
  expect(titleLineBoxes.at(-1)!.x + titleLineBoxes.at(-1)!.width).toBeGreaterThan(timeBox!.x);
  await expect(row.locator('[data-component="tag-chip"]')).toHaveCount(4);
  for (const chip of await row.locator('[data-component="tag-chip"]').all()) await expect(chip).toBeVisible();
});

test('presents note kinds and round-trips reader and Markdown editor compositions', async ({ page }) => {
  await page.goto('/ux-demo?component=note-card');
  const notes = page.locator('[data-component="note-card"]');
  await expect(notes).toHaveCount(5);
  for (const [kind, icon] of [['regular', 'message-square-text'], ['status', 'refresh-cw'], ['feedback_needed', 'circle-alert'], ['feedback_draft', 'file-pen-line'], ['activity', 'activity']] as const) {
    const note = notes.filter({ has: page.locator(`[data-lucide="${icon}"]`) });
    await expect(note).toHaveAttribute('data-kind', kind);
    await expect(note.locator(`[data-lucide="${icon}"]`)).toBeVisible();
  }
  const standaloneNote = notes.filter({ has: page.locator('[data-lucide="message-square-text"]') });
  await standaloneNote.dblclick();
  const standaloneEditor = standaloneNote.getByRole('textbox', { name: 'Note body' });
  await standaloneEditor.fill('Persisted standalone note');
  await standaloneEditor.blur();
  await expect(standaloneNote).toContainText('Persisted standalone note');
  await standaloneNote.dblclick();
  await standaloneNote.getByRole('textbox', { name: 'Note body' }).fill('Autosaved replacement');
  await standaloneNote.getByRole('textbox', { name: 'Note body' }).blur();
  await expect(standaloneNote).toContainText('Autosaved replacement');

  await page.goto('/ux-demo?component=ticket-reader');
  const reader = page.locator('[data-component="ticket-reader"]');
  await expect(reader.getByRole('heading', { name: 'Build TicketReader component and UX demo' })).toBeVisible();
  await expect(reader.locator('.ticket-inspector__details-surface [data-component="markdown-preview"]')).toContainText('Implementation notes');
  const readerGuide = reader.getByRole('link', { name: 'Open the component guide' });
  await expect(readerGuide).toHaveAttribute('target', '_blank');
  await expect(readerGuide).toHaveAttribute('rel', 'noopener noreferrer');
  const popupPromise = page.waitForEvent('popup');
  await readerGuide.click();
  const popup = await popupPromise;
  await expect(popup).toHaveURL(/component=tag-chip/);
  await popup.close();
  await expect(reader.locator('[data-component="note-card"]')).toHaveCount(5);
  const noteHistory = reader.getByRole('link', { name: 'note history' });
  await expect(noteHistory).toHaveAttribute('target', '_blank');
  await expect(noteHistory).toHaveAttribute('rel', 'noopener noreferrer');
  await reader.screenshot({ path: '/private/tmp/hs2-hnh0m6-markdown-links-wide.png' });
  await page.setViewportSize({ width: 940, height: 844 });
  await noteHistory.scrollIntoViewIfNeeded();
  await reader.screenshot({ path: '/private/tmp/hs2-hnh0m6-markdown-links-narrow.png' });
  await page.setViewportSize({ width: 1280, height: 720 });
  await expect(reader.getByRole('heading', { name: /Notes/ }).locator('span')).toHaveText('5');
  await expect(reader.locator('.ticket-inspector__content')).toHaveCSS('overflow-y', 'auto');
  const readerWidth = await reader.boundingBox();
  const readerContentWidth = await reader.locator('.ticket-inspector__content').boundingBox();
  expect(readerContentWidth!.width).toBeGreaterThan(readerWidth!.width * .9);
  const editableNote = reader.locator('[data-component="note-card"][data-note-id="reader-note"]');
  await expect(editableNote.locator('.note-card__body')).toHaveAttribute('aria-label', 'Edit note');
  await reader.getByRole('button', { name: 'Edit Ticket details' }).dblclick();
  const readerDetails = reader.getByRole('textbox', { name: 'Ticket details' });
  await expect(readerDetails).toBeFocused();
  await expect(readerDetails).toHaveCSS('resize', 'vertical');
  await editableNote.locator('.note-card__body').dblclick();
  await expect(editableNote.getByRole('textbox', { name: 'Note body' })).toBeFocused();
  await expect(editableNote.getByRole('textbox', { name: 'Note body' })).toHaveCSS('resize', 'vertical');
  await editableNote.getByRole('textbox', { name: 'Note body' }).fill('Edited note body');
  await editableNote.getByRole('textbox', { name: 'Note body' }).blur();
  await expect(editableNote).toContainText('Edited note body');
  await editableNote.dblclick();
  await editableNote.getByRole('textbox', { name: 'Note body' }).fill('Autosaved note body');
  await editableNote.getByRole('textbox', { name: 'Note body' }).blur();
  await expect(editableNote.getByRole('textbox', { name: 'Note body' })).toHaveCount(0);
  await expect(editableNote).toContainText('Autosaved note body');
  await reader.getByRole('button', { name: 'Attachments' }).click();
  await expect(reader.locator('[data-component="ticket-attachments"]')).toContainText('reader-wireframe.png');
  await reader.getByLabel('Browse and add attachments').setInputFiles({ name: 'browser-added.txt', mimeType: 'text/plain', buffer: Buffer.from('added') });
  await expect(reader.locator('[data-component="ticket-attachments"]')).toContainText('browser-added.txt');
  await reader.locator('[data-component="ticket-inspector"]').evaluate(node => { const transfer = new DataTransfer(); transfer.items.add(new File(['drop'], 'dropped.txt', { type: 'text/plain' })); node.dispatchEvent(new DragEvent('drop', { bubbles: true, cancelable: true, dataTransfer: transfer })); });
  await expect(reader.locator('[data-component="ticket-attachments"]')).toContainText('dropped.txt');
  await reader.getByRole('button', { name: 'Info' }).click();
  await expect(reader.locator('.ticket-inspector__details-section')).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
  await expect(reader.locator('.ticket-inspector__details-surface')).toHaveCSS('background-color', 'rgb(255, 255, 255)');
  await expect(reader.locator('.ticket-inspector__details-surface .markdown-editor--embedded')).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
  await expect(reader.getByRole('textbox', { name: 'Feedback response' })).toBeVisible();
  await expect(reader.getByRole('textbox', { name: 'Note body' })).toHaveValue(/keep the response/i);
  await reader.getByRole('button', { name: 'Edit Ticket details' }).dblclick();
  const readerSource = reader.getByRole('textbox', { name: 'Ticket details' });
  await readerSource.fill('## Reader draft\nPreserved across the shared inspector surface.');
  await readerSource.blur();
  await expect(reader.locator('.ticket-inspector__details-surface [data-component="markdown-preview"]')).toContainText('Reader draft');

  await page.goto('/ux-demo?component=markdown-editor');

  const editor = page.locator('[data-component="markdown-editor"]');
  await expect(editor.locator('[data-component="markdown-preview"]')).toContainText('Implementation notes');
  await expect(editor.getByRole('link', { name: 'Open the component guide' })).toHaveAttribute('target', '_blank');
  await editor.getByRole('button', { name: 'Edit Markdown content' }).dblclick();
  const source = editor.getByRole('textbox', { name: 'Markdown content' });
  await expect(source).toHaveValue(/Implementation notes/);
  await source.fill('## Revised goal\nA preserved draft.');
  await expect(editor.locator('footer')).toHaveCount(0);
  await editor.getByRole('button', { name: 'Expand editor' }).click();
  await expect(editor).toHaveAttribute('data-expanded', 'true');
  await expect(editor).toHaveCSS('position', 'fixed');
  await editor.getByRole('textbox', { name: 'Markdown content' }).focus();
  await editor.getByRole('textbox', { name: 'Markdown content' }).blur();
  await expect(editor).toHaveAttribute('data-mode', 'preview');
  await expect(editor.locator('[data-component="markdown-preview"]')).toContainText('Revised goal');
  await editor.getByRole('button', { name: 'Edit Markdown content' }).dblclick();
  await editor.getByRole('textbox', { name: 'Markdown content' }).fill('Autosaved edit');
  await editor.getByRole('textbox', { name: 'Markdown content' }).blur();
  await expect(editor).toHaveAttribute('data-mode', 'preview');
  await expect(editor.locator('[data-component="markdown-preview"]')).toContainText('Autosaved edit');
  await editor.getByRole('button', { name: 'Use inline editor' }).click();
  await expect(editor).toHaveAttribute('data-expanded', 'false');
});

test('keeps feedback Markdown list spacing compact', async ({ page }) => {
  await page.goto('/ux-demo?component=note-card');
  const feedbackNote = page.locator('[data-component="note-card"][data-kind="feedback_needed"]');
  const feedbackItems = feedbackNote.locator('li');
  await expect(feedbackItems).toHaveCount(3);
  const itemGaps = await feedbackItems.evaluateAll(items => items.slice(1).map((item, index) => item.getBoundingClientRect().top - items[index].getBoundingClientRect().bottom));
  expect(Math.max(...itemGaps)).toBeLessThanOrEqual(8);
  await feedbackNote.screenshot({ path: '/private/tmp/hs2-8dd2dg-feedback-list-spacing.png' });
});

test('spaces paragraphs and de-emphasizes email-style quoted Markdown at wide and narrow sizes', async ({ page }) => {
  await page.goto('/ux-demo?component=markdown-editor');
  const preview=page.locator('[data-component="markdown-preview"]'),quote=preview.locator('blockquote'),body=preview.locator(':scope > p').first();
  await expect(quote).toBeVisible();
  const typography=await Promise.all([quote,body].map(locator=>locator.evaluate(node=>({fontSize:parseFloat(getComputedStyle(node).fontSize),lineHeight:parseFloat(getComputedStyle(node).lineHeight),marginLeft:getComputedStyle(node).marginLeft}))));
  expect(typography[0].fontSize).toBeLessThan(typography[1].fontSize);expect(typography[0].lineHeight).toBeLessThan(typography[1].lineHeight);expect(typography[0].marginLeft).toBe('0px');
  await expect(body).toHaveCSS('margin-top','16px');await expect(body).toHaveCSS('margin-bottom','16px');
  await preview.screenshot({path:'/private/tmp/hs2-9acety-quoted-content-wide.png'});await page.setViewportSize({width:390,height:844});await quote.scrollIntoViewIfNeeded();await preview.screenshot({path:'/private/tmp/hs2-9acety-quoted-content-narrow.png'});
});

test('uses the identical responsive TicketRow in list and board compositions', async ({ page }) => {
  await page.setViewportSize({ width: 1600, height: 900 });
  await page.goto('/ux-demo?component=ticket-list');
  const list = page.getByRole('listbox', { name: 'Example ticket list' });
  const listRows = list.locator('[data-component="ticket-list-row"]');
  await expect(listRows).toHaveCount(20);
  const emptyExamples=page.getByLabel('TicketList empty states');await expect(emptyExamples.getByText('No tickets yet')).toBeVisible();await expect(emptyExamples.getByText('No tickets in Backlog')).toBeVisible();await expect(emptyExamples.getByText('No tickets match “parser”')).toBeVisible();
  await expect(list.locator('..')).toHaveCSS('border-radius', '10.4px');
  const listRow = listRows.first();
  await expect(listRow).toHaveAttribute('data-presentation', 'list');
  const listIdentity = listRow.locator('.ticket-list-row__identity');
  const listTitleMetrics = await listIdentity.evaluate(node => {
    const style = getComputedStyle(node);
    return { lineHeight: Number.parseFloat(style.lineHeight), maxHeight: Number.parseFloat(style.maxHeight) };
  });
  expect(listTitleMetrics.maxHeight / listTitleMetrics.lineHeight).toBeCloseTo(2, 1);
  const listWidth = await listRow.evaluate(node => node.getBoundingClientRect().width);
  expect(listWidth).toBeGreaterThan(600);
  const [listBox, listHostBox] = await Promise.all([list.boundingBox(), list.locator('xpath=..').boundingBox()]);
  expect(listBox).not.toBeNull(); expect(listHostBox).not.toBeNull();
  expect(listBox!.width).toBeCloseTo(listHostBox!.width, 0);
  await expect(listRows.first()).toHaveCSS('border-radius', '10.4px 10.4px 0px 0px');
  await expect(listRows.nth(1)).toHaveCSS('border-radius', '0px');
  await expect(listRows.last()).toHaveCSS('border-radius', '0px 0px 10.4px 10.4px');
  await list.evaluate(node => { (node.parentElement as HTMLElement).style.width = '320px'; });
  await expect(listRows.first()).toHaveCSS('border-radius', '10.4px 10.4px 0px 0px');
  await expect(listRows.nth(1)).toHaveCSS('border-radius', '0px');
  await expect(listRows.last()).toHaveCSS('border-radius', '0px 0px 10.4px 10.4px');
  await list.screenshot({ path: '/private/tmp/hs2-y4de25-narrow-list-edge-rounding.png' });
  await list.evaluate(node => { (node.parentElement as HTMLElement).style.width = ''; });
  await expect(listRow).toHaveCSS('box-shadow', 'none');
  await listRow.click();
  await expect(listRow).toHaveAttribute('data-selected', 'true');
  await expect(page.getByText('1 ticket selected')).toBeVisible();
  await listRows.nth(1).click({ modifiers: ['Meta'] });
  await expect(listRows.nth(0)).toHaveAttribute('data-selected', 'true');
  await expect(listRows.nth(1)).toHaveAttribute('data-selected', 'true');
  await expect(page.getByText('2 tickets selected')).toBeVisible();
  await listRows.nth(3).click({ modifiers: ['Shift'] });
  await expect(listRows.nth(1)).toHaveAttribute('data-selected', 'true');
  await expect(listRows.nth(2)).toHaveAttribute('data-selected', 'true');
  await expect(listRows.nth(3)).toHaveAttribute('data-selected', 'true');
  await expect(listRows.nth(0)).toHaveAttribute('data-selected', 'false');
  const listStar = listRow.locator('[data-action="toggle-row-up-next"]');
  await listStar.click();
  await expect(listStar).not.toHaveClass(/active/);
  await expect(page.getByText('HS2-R76MMW removed from Up Next')).toBeVisible();

  await page.locator('.demo-master [data-item-id="ticket-board"]').click();
  await expect(page).toHaveURL('/ux-demo?component=ticket-board');
  const board = page.getByRole('listbox', { name: 'Example status board' });
  await expect(board.locator('.ticket-board-column')).toHaveCount(3);
  const emptyBoard=page.getByRole('listbox',{name:'Empty search board'});await expect(emptyBoard.locator('[data-component="ticket-empty-state"]')).toHaveCount(1);await expect(emptyBoard.getByText('No tickets match “parser”')).toBeVisible();
  expect(await board.locator('.ticket-board-column__header').evaluateAll(headers => headers.map(header => header.getBoundingClientRect().height))).toEqual([32, 32, 32]);
  await expect(board.locator('.ticket-board-column').first()).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
  await expect(board.locator('.ticket-board-column').first()).toHaveCSS('padding', '0px');
  await expect(board.locator('.ticket-board-column__tickets').first()).toHaveCSS('padding', '1.6px 8px 16px');
  await expect(board).toHaveCSS('padding', '0px 8px');
  await expect(board.locator('.ticket-board__columns')).toHaveCSS('gap', '0px');
  await expect(board).toHaveCSS('border-top-width', '0px');
  await expect(board).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
  await expect(board.getByLabel('6 tickets')).toHaveCount(1);
  await expect(board.getByLabel('7 tickets')).toHaveCount(2);
  const boardRows = board.locator('[data-component="ticket-list-row"]');
  await expect(boardRows).toHaveCount(20);
  const scrollRegions = board.locator('.ticket-board-column__tickets');
  await expect(scrollRegions).toHaveCount(3);
  const initialScroll = await scrollRegions.evaluateAll(regions => regions.map(region => ({ clientHeight: region.clientHeight, scrollHeight: region.scrollHeight, scrollTop: region.scrollTop })));
  expect(initialScroll.every(region => region.scrollHeight > region.clientHeight)).toBe(true);
  const firstHeaderTop = await board.locator('.ticket-board-column__header').first().evaluate(node => node.getBoundingClientRect().top);
  await scrollRegions.first().evaluate(node => { node.scrollTop = 180; node.dispatchEvent(new Event('scroll')); });
  await expect.poll(() => scrollRegions.first().evaluate(node => node.scrollTop)).toBeGreaterThan(0);
  expect(await scrollRegions.nth(1).evaluate(node => node.scrollTop)).toBe(0);
  expect(await board.locator('.ticket-board-column__header').first().evaluate(node => node.getBoundingClientRect().top)).toBeCloseTo(firstHeaderTop, 0);
  const narrowRow = boardRows.first();
  await expect(narrowRow).toHaveAttribute('data-presentation', 'column');
  const inlineCategory = narrowRow.locator('.ticket-list-row__category');
  const inlineCategorySize = await inlineCategory.evaluate(node => ({ width: node.getBoundingClientRect().width, height: node.getBoundingClientRect().height }));
  expect(inlineCategorySize.width).toBeCloseTo(17.6, 1);
  expect(inlineCategorySize.height).toBeCloseTo(17.6, 1);
  const inlineOrder = await narrowRow.locator('.ticket-list-row__identity').evaluate(node => [...node.children].map(child => child.className));
  expect(inlineOrder[1]).toContain('ticket-list-row__category');
  expect(inlineOrder[2]).toContain('ticket-list-row__slug');
  const [categoryBox, slugBox] = await Promise.all([inlineCategory.boundingBox(), narrowRow.locator('.ticket-list-row__slug').boundingBox()]);
  expect(categoryBox).not.toBeNull(); expect(slugBox).not.toBeNull();
  expect(Math.abs(categoryBox!.y + categoryBox!.height / 2 - (slugBox!.y + slugBox!.height / 2))).toBeLessThanOrEqual(1.5);
  const columnIdentity = narrowRow.locator('.ticket-list-row__identity');
  const columnTitleMetrics = await columnIdentity.evaluate(node => {
    const style = getComputedStyle(node);
    return { lineHeight: Number.parseFloat(style.lineHeight), maxHeight: Number.parseFloat(style.maxHeight) };
  });
  expect(columnTitleMetrics.maxHeight / columnTitleMetrics.lineHeight).toBeCloseTo(4, 1);
  const longColumnRow = board.locator('[data-ticket-slug="HS2-SG1BKJ"]');
  const longColumnTitle = longColumnRow.locator('.ticket-list-row__identity strong');
  await longColumnRow.scrollIntoViewIfNeeded();
  expect(await longColumnTitle.evaluate(node => node.getClientRects().length)).toBe(4);
  await longColumnRow.screenshot({ path: '/private/tmp/hs2-0tfhhs-four-line-column-title-wide.png' });
  const boardWidth = await narrowRow.evaluate(node => node.getBoundingClientRect().width);
  expect(boardWidth).toBeLessThan(384);
  await expect(narrowRow).toHaveCSS('border-radius', '10.4px');
  await expect(narrowRow).toHaveCSS('box-shadow', 'none');
  await expect(page.locator('[data-component="ticket-card"]')).toHaveCount(0);
  await board.getByRole('button', { name: 'Select all Backlog tickets' }).click();
  await expect(board.locator('[data-column-id="backlog"] [data-selected="true"]')).toHaveCount(6);
  await page.screenshot({ path: '/private/tmp/hs2-x91ssp-column-select-wide.png', fullPage: true });
  await page.screenshot({ path: '/private/tmp/hs2-xrnsv0-column-header-wide.png', fullPage: true });
  await page.screenshot({ path: '/private/tmp/hs2-4gk04w-column-row-wide.png', fullPage: true });
  await page.setViewportSize({ width: 760, height: 900 });
  expect(await board.locator('.ticket-board-column__header').evaluateAll(headers => headers.map(header => header.getBoundingClientRect().height))).toEqual([32, 32, 32]);
  await page.screenshot({ path: '/private/tmp/hs2-xrnsv0-column-header-narrow.png', fullPage: true });
  await page.screenshot({ path: '/private/tmp/hs2-4gk04w-column-row-narrow.png', fullPage: true });
  await page.setViewportSize({ width: 1600, height: 900 });
  await narrowRow.focus();
  await page.keyboard.press('Meta+A');
  await expect(board.locator('[data-selected="true"]')).toHaveCount(20);
  await page.keyboard.press('Enter');
  await expect(board.locator('[data-selected="true"]')).toHaveCount(1);
  await expect(narrowRow).toHaveAttribute('data-selected', 'true');
  // Right-click an Up-Next-eligible (started) row: the context menu only offers
  // "Toggle Up Next" for not_started/started tickets, so a backlog row (boardRows.first())
  // correctly hides it — this must exercise an eligible row (HS2-AFB17W).
  const eligibleRow = board.locator('[data-ticket-slug="HS2-R76MMW"]');
  await expect(eligibleRow).toHaveAttribute('data-status', 'started');
  await eligibleRow.click({ button: 'right' });
  const menu = page.getByRole('menu', { name: 'Ticket actions' });
  await expect(menu).toBeVisible();
  await menu.getByText('Toggle Up Next', { exact: true }).click();
  await expect(page.getByText(/Toggle Up Next selected for HS2-R76MMW/)).toBeVisible();
  await page.goto('/ux-demo?component=ticket-board-column');
  const columnStage = page.locator('.collection-demo--column');
  const columnDemo = page.locator('[data-component="ticket-board-column"]');
  await expect(columnStage).toHaveCSS('min-width', '250px');
  expect((await columnDemo.first().boundingBox())!.width).toBeGreaterThanOrEqual(250);
  await expect(columnDemo).toHaveCount(1);
  await expect(columnDemo.first().getByLabel('7 tickets')).toBeVisible();
  await expect(columnDemo.first().locator('.ticket-board-column__tickets')).toHaveCSS('overflow-y', 'auto');
});

test('omits status sorting from column view and restores it in list view', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto('/ux-demo?component=workspace-header');
  const header = page.locator('[data-component="workspace-header"]');
  const sortSelect = header.locator('wa-select[name="workspace-sort"]');
  await expect(sortSelect.locator('wa-option[value="status"]')).toHaveCount(1);
  await header.getByRole('button', { name: 'Columns view' }).click();
  await expect(sortSelect.locator('wa-option[value="status"]')).toHaveCount(0);
  await sortSelect.click();
  await expect(sortSelect).toHaveJSProperty('open', true);
  await page.screenshot({ path: '/private/tmp/hs2-nydfqf-column-sort-options.png', fullPage: true });
});

test('switches and searches the connected workspace through WorkspaceHeader', async ({ page }) => {
  await page.setViewportSize({ width: 1600, height: 900 });
  await page.goto('/ux-demo?component=workspace-header');
  const header = page.locator('[data-component="workspace-header"]');
  await expect(header).toContainText('Hot Sheet 2');
  const sortIcon=header.locator('.workspace-header__sort .select__custom-selected'),idleViewIcon=header.getByRole('button',{name:'Columns view'}),quietIconColor=await idleViewIcon.evaluate(node=>getComputedStyle(node).color);
  expect(await sortIcon.evaluate(node=>getComputedStyle(node).color)).toBe(quietIconColor);
  const notificationBadge = header.locator('.view-mode-switcher__badge');
  await expect(notificationBadge).toHaveText('7');
  await expect(notificationBadge).toHaveCSS('font-size', '10px');
  await expect(notificationBadge).toHaveCSS('padding', '1px 5px');
  await expect(notificationBadge).toHaveCSS('background-color', 'rgb(255, 204, 0)');
  await notificationBadge.screenshot({ path: '/private/tmp/hs2-x9embf-notification-badge.png' });
  await page.screenshot({ path: '/private/tmp/hs2-rza0h3-semantic-tokens-wide.png', fullPage: true });
  await expect(header.getByRole('button', { name: 'List view' })).toHaveAttribute('aria-pressed', 'true');
  await expect(page.getByRole('listbox', { name: 'Workspace tickets' }).locator('[data-component="ticket-list-row"]')).toHaveCount(20);
  await header.getByRole('button', { name: 'Columns view' }).click();
  await expect(header.getByRole('button', { name: 'Columns view' })).toHaveAttribute('aria-pressed', 'true');
  await expect(page.getByRole('listbox', { name: 'Workspace board' })).toBeVisible();
  const closedHeaderHeight = await header.evaluate(node => node.getBoundingClientRect().height);
  const searchGroup = header.locator('.workspace-header__search-group');
  const collapsedWidth = await searchGroup.evaluate(node => node.getBoundingClientRect().width);
  const collapsedHeight = await searchGroup.evaluate(node => node.getBoundingClientRect().height);
  expect(collapsedWidth).toBeCloseTo(collapsedHeight, 0);
  const findButton = header.getByRole('button', { name: 'Search tickets' });
  await findButton.click();
  await expect(findButton).toHaveCount(0);
  const searchControl = header.locator('wa-input[name="workspace-search"]');
  const search = header.getByRole('textbox', { name: 'Search tickets' });
  await expect(search).toBeFocused();
  await expect(searchControl.locator('[data-lucide="search"]')).toBeVisible();
  await expect(searchGroup).not.toHaveCSS('box-shadow', 'none');
  await expect.poll(() => searchGroup.evaluate(node => node.getBoundingClientRect().width)).toBeGreaterThan(collapsedWidth * 3);
  const openHeaderHeight = await header.evaluate(node => node.getBoundingClientRect().height);
  expect(Math.abs(openHeaderHeight - closedHeaderHeight)).toBeLessThanOrEqual(3);
  await search.fill('long-tag-example');
  await expect(page.getByRole('listbox', { name: 'Workspace board' }).locator('[data-component="ticket-list-row"]')).toHaveCount(1);
  // The X clear button empties the search and restores every row (HS2-Z7KP1Q).
  const clearSearch = header.locator('[data-action="clear-workspace-search"]');
  await expect(clearSearch).toBeVisible();
  await clearSearch.click();
  await expect(search).toHaveValue('');
  await expect(page.getByRole('listbox', { name: 'Workspace board' }).locator('[data-component="ticket-list-row"]')).toHaveCount(20);
  await expect(clearSearch).toHaveCount(0);
  await search.fill('long-tag-example');
  await header.getByRole('button', { name: 'Columns view' }).focus();
  await expect(search).toBeVisible();
  await search.fill('');
  await header.getByRole('button', { name: 'Columns view' }).focus();
  await expect(header.getByRole('textbox', { name: 'Search tickets' })).toHaveCount(0);
  await expect(header.getByRole('button', { name: 'Search tickets' })).toBeVisible();
  await expect(page.getByRole('listbox', { name: 'Workspace board' }).locator('[data-component="ticket-list-row"]')).toHaveCount(20);
  await header.getByRole('button', { name: 'List view' }).click();
  const sortSelect = header.locator('wa-select[name="workspace-sort"]');
  await expect(sortSelect).toHaveAttribute('aria-label', 'Sort tickets: Recently updated, descending');
  await expect(sortSelect.locator('.select__custom-selected [data-lucide="clock-arrow-down"]')).toBeVisible();
  await expect(sortSelect.locator('.select__custom-selected')).toHaveCSS('color',quietIconColor);
  const triggerGeometry=await sortSelect.evaluate(node=>{const root=node.shadowRoot!,combobox=root.querySelector<HTMLElement>('[part~="combobox"]')!,expand=root.querySelector<HTMLElement>('[part~="expand-icon"]')!,selected=node.querySelector<HTMLElement>('.select__custom-selected')!,outer=combobox.getBoundingClientRect(),icon=selected.getBoundingClientRect(),arrow=expand.getBoundingClientRect();return{width:outer.width,gap:arrow.left-icon.right,arrowOverflow:arrow.right-outer.right}});expect(triggerGeometry.width).toBeLessThanOrEqual(46);expect(triggerGeometry.gap).toBeLessThanOrEqual(8);expect(triggerGeometry.arrowOverflow).toBeLessThanOrEqual(0);
  await sortSelect.click();
  await expect(sortSelect.locator('wa-option[value="updated"] [data-lucide="arrow-down"]')).toBeVisible();
  await expect(sortSelect.locator('wa-option[value="priority"] .select__icon')).toBeVisible();
  await page.screenshot({ path: '/private/tmp/hs2-0dcczk-sort-select-wide.png', fullPage: true });
  const prioritySort = sortSelect.locator('wa-option[value="priority"]');
  await prioritySort.click();
  await expect(page.getByText('Sorted by priority, ascending')).toBeVisible();
  await expect(sortSelect).toHaveJSProperty('value', 'priority');
  await expect(sortSelect.locator('.select__custom-selected [data-lucide="arrow-up-narrow-wide"]')).toBeVisible();
  const ascendingPriorities = await page.getByRole('listbox', { name: 'Workspace tickets' }).locator('.ticket-list-row__priority').evaluateAll(nodes => nodes.map(node => node.getAttribute('aria-label')?.split(' ')[0]));
  expect(ascendingPriorities.map(value => ['low','default','high','urgent'].indexOf(value ?? ''))).toEqual([...ascendingPriorities].map(value => ['low','default','high','urgent'].indexOf(value ?? '')).sort((a,b)=>a-b));
  await page.screenshot({path:'/private/tmp/hs2-5avfng-priority-ascending.png',fullPage:true});
  const ascendingTitles = await page.getByRole('listbox', { name: 'Workspace tickets' }).locator('.ticket-list-row__identity strong').allTextContents();
  await sortSelect.click();
  await prioritySort.click();
  await expect(page.getByText('Sorted by priority, descending')).toBeVisible();
  await expect(sortSelect.locator('.select__custom-selected [data-lucide="arrow-down-wide-narrow"]')).toBeVisible();
  const descendingTitles = await page.getByRole('listbox', { name: 'Workspace tickets' }).locator('.ticket-list-row__identity strong').allTextContents();
  expect(descendingTitles).toEqual([...ascendingTitles].reverse());
  const statusOption=sortSelect.locator('wa-option[value="status"]');if(!await statusOption.isVisible())await sortSelect.click();await expect(statusOption).toBeVisible();await statusOption.click();const ascendingStatuses=await page.getByRole('listbox',{name:'Workspace tickets'}).locator('[data-component="ticket-list-row"]').evaluateAll(nodes=>nodes.map(node=>(node as HTMLElement).dataset.status));expect(ascendingStatuses.map(value=>['backlog','not_started','started','completed','verified','archive'].indexOf(value??''))).toEqual([...ascendingStatuses].map(value=>['backlog','not_started','started','completed','verified','archive'].indexOf(value??'')).sort((a,b)=>a-b));await page.screenshot({path:'/private/tmp/hs2-4penqq-status-ascending.png',fullPage:true});
  for (const [value,firstIcon,secondIcon] of [['title','arrow-down-a-z','arrow-up-a-z'],['updated','clock-arrow-down','clock-arrow-up']] as const) {
    const option=sortSelect.locator(`wa-option[value="${value}"]`);await sortSelect.click();await expect(option).toBeVisible();await option.click();await expect(option).not.toBeVisible();await expect(sortSelect.locator(`.select__custom-selected [data-lucide="${firstIcon}"]`)).toBeVisible();await sortSelect.click();await expect(option).toBeVisible();await option.click();await expect(option).not.toBeVisible();await expect(sortSelect.locator(`.select__custom-selected [data-lucide="${secondIcon}"]`)).toBeVisible();
  }
  await page.setViewportSize({width:1024,height:600});await sortSelect.click();await page.screenshot({path:'/private/tmp/hs2-0dcczk-sort-select-floor.png',fullPage:true});await page.keyboard.press('Escape');
  await header.getByRole('button', { name: 'Settings view' }).click();
  await expect(header.getByRole('button', { name: 'Settings view' })).toHaveAttribute('aria-pressed', 'true');
  await expect(sortSelect).toHaveAttribute('disabled', '');
  for (const name of ['Toggle Up Next for selected tickets', 'More actions for selected tickets', 'Search tickets']) await expect(header.getByRole('button', { name })).toHaveAttribute('disabled', '');
  await expect(page.getByRole('region', { name: 'Project settings' })).toBeVisible();
  await expect(page.getByRole('region', { name: 'Workspace board' })).toHaveCount(0);
  await header.getByRole('button', { name: 'List view' }).click();
  await expect(page.getByRole('listbox', { name: 'Workspace tickets' })).toBeVisible();
  await page.setViewportSize({ width: 760, height: 900 });
  await expect(notificationBadge).toHaveCSS('font-size', '10px');
  await page.screenshot({ path: '/private/tmp/hs2-rza0h3-semantic-tokens-narrow.png', fullPage: true });
});

test('organizes search syntax help in the WorkspaceHeader demo',async({page})=>{
  await page.setViewportSize({width:1440,height:900});
  await page.goto('/ux-demo?component=workspace-header');
  const header=page.locator('[data-component="workspace-header"]');
  await header.getByRole('button',{name:'Search tickets'}).click();
  await header.getByRole('textbox',{name:'Search tickets'}).fill('client');
  await header.getByRole('button',{name:'Search syntax help'}).click();
  const help=header.getByRole('dialog',{name:'Search syntax'});
  await expect(help.locator('dt')).toHaveText(['Tags','Content','Workflow','Dates']);
  await expect(help).toContainText('Combine filters');
  await page.screenshot({path:'/private/tmp/hs2-7efj3e-search-help-demo-wide.png',fullPage:true});
  await page.setViewportSize({width:760,height:640});
  await page.screenshot({path:'/private/tmp/hs2-7efj3e-search-help-demo-narrow.png',fullPage:true});
  await expect.poll(()=>help.evaluate(node=>{const box=node.getBoundingClientRect();return box.left>=0&&box.right<=innerWidth&&box.bottom<=innerHeight})).toBe(true);
  await header.getByRole('button',{name:'Search syntax help'}).click();
  await expect(help).toHaveCount(0);
});

test('shows the ToolbarControlGroup variants with shared geometry', async ({ page }) => {
  await page.goto('/ux-demo?component=toolbar-control-group');
  const demo = page.getByRole('region', { name: 'ToolbarControlGroup demo' });
  const groups = demo.locator('.toolbar-control-group');
  await expect(groups).toHaveCount(8);
  for (const icon of ['arrow-down-a-z', 'star', 'ellipsis', 'pin', 'panel-left-open']) await expect(demo.locator(`[data-lucide="${icon}"]`)).toBeVisible();
  const heights = await groups.evaluateAll(nodes => nodes.map(node => node.getBoundingClientRect().height));
  expect(new Set(heights).size).toBe(1);
  const popup = demo.locator('wa-button[with-caret]');
  const caretSpacing = await popup.evaluate(node => {
    const button = node.shadowRoot?.querySelector<HTMLElement>('[part~="button"]');
    const caret = node.shadowRoot?.querySelector<HTMLElement>('[part~="caret"]');
    return button && caret ? { gap: getComputedStyle(button).gap, margin: getComputedStyle(caret).marginInlineStart, width: button.getBoundingClientRect().width, height: button.getBoundingClientRect().height } : null;
  });
  expect(caretSpacing).toEqual({ gap: '2px', margin: '2px', width: 48, height: 32 });
  const popupGroupWidth = await groups.nth(1).evaluate(node => node.getBoundingClientRect().width);
  expect(popupGroupWidth - caretSpacing!.width).toBeCloseTo(8.4, 0);
  await popup.hover();
  await expect(groups.nth(1)).toHaveCSS('background-color', 'rgb(255, 255, 255)');
  const popupBackground = await popup.evaluate(node => {
    const button = node.shadowRoot?.querySelector<HTMLElement>('[part~="base"]');
    return button ? getComputedStyle(button).backgroundColor : null;
  });
  expect(popupBackground).toBe('rgba(0, 0, 0, 0)');
  const groupedButton = demo.locator('wa-button[aria-label="Favorite view"]').first();
  await groupedButton.hover();
  const groupedGeometry = await groupedButton.evaluate(node => {
    const button = node.shadowRoot?.querySelector<HTMLElement>('[part~="base"]');
    if (!button) return null;
    return { height: button.getBoundingClientRect().height, background: getComputedStyle(node).backgroundColor };
  });
  expect(groupedGeometry).toEqual({ height: 32, background: 'rgb(255, 255, 255)' });
  const iconAlignment = await groupedButton.evaluate(node => {
    const button = node.shadowRoot?.querySelector<HTMLElement>('[part~="base"]');
    const icon = node.querySelector<HTMLElement>('[data-lucide]');
    if (!button || !icon) return null;
    const buttonBox = button.getBoundingClientRect(); const iconBox = icon.getBoundingClientRect();
    return Math.abs((buttonBox.top + buttonBox.height / 2) - (iconBox.top + iconBox.height / 2));
  });
  expect(iconAlignment).toBeLessThan(1);
  const borderless = groups.nth(4);
  await expect(borderless).toHaveAttribute('data-appearance', 'borderless');
  await expect(borderless).toHaveCSS('border-color', 'rgba(0, 0, 0, 0)');
  await expect(borderless).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
  await borderless.getByRole('button').hover();
  await expect(borderless).toHaveCSS('background-color', 'rgb(255, 255, 255)');
  const restingPush = demo.getByRole('button', { name: 'Resting comparison' }).locator('..');
  const pressedPush = demo.getByRole('button', { name: 'Pressed comparison' }).locator('..');
  await expect(restingPush).toHaveAttribute('data-button-appearance', 'push');
  await expect(pressedPush).toHaveCSS('background-color', 'rgb(72, 72, 74)');
  await expect(pressedPush).toHaveCSS('border-color', 'rgb(174, 174, 178)');
  await expect(pressedPush.getByRole('button')).toHaveCSS('color', 'rgb(255, 255, 255)');
  const dark = demo.getByRole('group', { name: 'Dark navigation' });
  await expect(dark).toHaveAttribute('data-tone', 'dark');
  await expect(dark).toHaveCSS('color', 'rgb(255, 255, 255)');
  await expect(dark).toHaveCSS('border-color', 'rgb(53, 53, 54)');
  await page.screenshot({ path: '/private/tmp/hs2-t3m818-dark-toolbar-border-wide.png', fullPage: true });
  await page.setViewportSize({ width: 760, height: 900 });
  await expect(dark).toHaveCSS('border-color', 'rgb(53, 53, 54)');
  await page.screenshot({ path: '/private/tmp/hs2-t3m818-dark-toolbar-border-narrow.png', fullPage: true });
  await expect(demo.getByRole('heading', { name: 'Single button' })).toBeVisible();
});

test('shows the reader text push state at exactly one and a half times normal size', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto('/ux-demo?component=ticket-reader');
  const reader = page.locator('[data-component="ticket-reader"]');
  const paragraph = reader.locator('.ticket-inspector__details-surface .markdown-preview p').first();
  const ordinarySize = parseFloat(await paragraph.evaluate(node => getComputedStyle(node).fontSize));
  const toggle = reader.locator('[data-action="toggle-reader-text-size"]');
  await expect(toggle).toHaveAccessibleName('Use large reader text size');
  const pushGroup = toggle.locator('..');
  await expect(pushGroup).toHaveAttribute('data-button-appearance', 'push');
  await toggle.click();
  await expect(reader).toHaveAttribute('data-large-text', 'true');
  await expect(toggle).toHaveAttribute('aria-pressed', 'true');
  await expect(toggle).toHaveAccessibleName('Use standard reader text size');
  await expect.poll(async () => parseFloat(await paragraph.evaluate(node => getComputedStyle(node).fontSize))).toBeCloseTo(ordinarySize * 1.5, 1);
  await expect(pushGroup).toHaveCSS('background-color', 'rgb(72, 72, 74)');
  await expect(pushGroup).toHaveCSS('border-color', 'rgb(174, 174, 178)');
  await expect(toggle).toHaveCSS('color', 'rgb(255, 255, 255)');
  await page.screenshot({ path: '/private/tmp/hs2-28frr0-reader-pressed-wide.png', fullPage: true });
  await reader.locator('.ticket-inspector__header').screenshot({ path: '/private/tmp/hs2-28frr0-reader-pressed-header.png' });
  await page.setViewportSize({ width: 1024, height: 700 });
  await reader.locator('[data-component="ticket-notes"]').scrollIntoViewIfNeeded();
  await page.screenshot({ path: '/private/tmp/hs2-28frr0-reader-pressed-narrow.png', fullPage: true });
  await toggle.click();
  await expect(reader).toHaveAttribute('data-large-text', 'false');
});

test('shows repository comparison as a shared pressed toolbar control', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto('/ux-demo?component=repository-status-popover');
  const dialog = page.locator('[data-component="repository-status-popover"]');
  await dialog.getByRole('button', { name: /Commits/ }).click();
  const compare = dialog.getByRole('button', { name: 'Compare two commits' });
  const group = compare.locator('..');
  await expect(group).toHaveAttribute('data-button-appearance', 'push');
  await compare.click();
  await expect(compare).toHaveAttribute('aria-pressed', 'true');
  await expect(group).toHaveCSS('background-color', 'rgb(72, 72, 74)');
  await expect(group).toHaveCSS('border-color', 'rgb(174, 174, 178)');
  await expect(compare).toHaveCSS('color', 'rgb(255, 255, 255)');
  await dialog.screenshot({ path: '/private/tmp/hs2-7cnf5b-compare-pressed-wide.png' });
  await dialog.locator('[data-component="dialog-header"]').screenshot({ path: '/private/tmp/hs2-7cnf5b-compare-pressed-header.png' });
  await page.addStyleTag({ content: 'body{min-width:0}.demo-shell{display:block}.demo-master,.demo-detail__header,.demo-detail__footer,.settings-toggle{display:none}.demo-detail{min-height:0;padding:12px}' });
  await page.setViewportSize({ width: 760, height: 640 });
  await page.mouse.move(740, 620);
  await page.waitForTimeout(150);
  await dialog.screenshot({ path: '/private/tmp/hs2-7cnf5b-compare-pressed-narrow.png' });
  await compare.click();
  await expect(compare).toHaveAttribute('aria-pressed', 'false');
});

test('expands, validates, creates, and cancels through QuickTicketComposer', async ({ page }) => {
  await page.goto('/ux-demo?component=quick-ticket-composer');
  await page.getByRole('button', { name: /New ticket/ }).click();
  const form = page.locator('[data-action="create-ticket-form"]');
  const title = form.getByRole('textbox', { name: 'Ticket title' });
  await expect(title).toBeFocused();
  await form.getByRole('button', { name: 'Create ticket' }).click();
  expect(await title.evaluate((node: HTMLInputElement) => node.checkValidity())).toBe(false);
  await expect(form).toBeVisible();
  await title.fill('Created from the UX demo');
  const details=form.getByRole('textbox',{name:'Details'});await details.fill('One-line details that can grow.');expect(await details.getAttribute('rows')).toBe('1');expect(await details.evaluate(node=>getComputedStyle(node).resize)).toBe('vertical');
  const category = form.locator('wa-select[name="new-ticket-category"]');
  await expect(category.locator('.select__icon--selected [data-lucide="list-checks"]')).toBeVisible();
  await category.click();
  const selectedOption = category.locator('wa-option[value="task"]');
  await expect(selectedOption).toHaveCSS('background-color', 'color(srgb 0.84 0.925333 1)');
  await expect(selectedOption).toHaveCSS('color', 'rgb(30, 110, 244)');
  await expect(selectedOption.locator('.select__icon')).toHaveCSS('color', 'rgb(20, 184, 166)');
  await expect(category.locator('wa-option[value="bug"] [data-lucide="bug"]')).toBeVisible();
  await page.keyboard.press('Escape');
  await category.evaluate((node: HTMLElement & { value: string }) => { node.value = 'bug'; node.dispatchEvent(new Event('change', { bubbles: true })); });
  await expect(form.locator('.select__icon--selected [data-lucide="bug"]')).toBeVisible();
  await form.getByRole('button',{name:'Add new ticket to Up Next'}).click();
  await expect(form.getByRole('button',{name:'Remove new ticket from Up Next'})).toHaveAttribute('aria-pressed','true');
  await form.screenshot({path:'/private/tmp/hs2-new-ticket-details-up-next.png'});
  await form.getByRole('button', { name: 'Create ticket' }).click();
  const createdList = page.getByRole('listbox', { name: 'Recently updated tickets' });
  await expect(createdList).toContainText('Created from the UX demo');
  await expect(createdList.locator('[data-component="ticket-list-row"]').first().locator('[data-lucide="bug"]')).toBeVisible();
  await expect(createdList.locator('[data-component="ticket-list-row"]').first().getByRole('button',{name:'Remove from Up Next'})).toBeVisible();
  await expect(page.getByText(/HS2-DEMO\d created/)).toBeVisible();
  await page.getByRole('button', { name: /New ticket/ }).click();
  await page.getByRole('textbox', { name: 'Ticket title' }).fill('Discard this');
  const cancel = page.getByRole('button', { name: /Cancel/ });
  await expect(cancel.locator('[data-lucide="x"]')).toHaveCount(0);
  await cancel.click();
  await expect(page.getByText('Ticket creation cancelled')).toBeVisible();
  await expect(page.getByRole('textbox', { name: 'Ticket title' })).toHaveCount(0);
  const launcher = page.getByRole('button', { name: /New ticket/ });
  await expect(launcher).toHaveCSS('cursor', 'pointer');
  await launcher.click();
  await expect(page.getByRole('textbox', { name: 'Ticket title' })).toBeFocused();
  await expect(page.getByRole('textbox',{name:'Details'})).toHaveValue('');
  await expect(page.getByRole('button',{name:'Add new ticket to Up Next'})).toHaveAttribute('aria-pressed','false');
});

test('navigates, toggles, closes, and reopens TicketInspector', async ({ page }) => {
  await page.goto('/ux-demo?component=ticket-inspector');
  const inspector = page.locator('[data-component="ticket-inspector"]');
  await expect(inspector).toContainText('Build TicketList and TicketBoard');
  const detailsParagraph = inspector.locator('.ticket-inspector__details-surface .markdown-preview > p').first();
  await expect(detailsParagraph).toHaveCSS('margin-top', '16px');
  await expect(detailsParagraph).toHaveCSS('margin-bottom', '16px');
  await inspector.getByRole('heading', { name: /Build TicketList/ }).dblclick();
  const titleEditor = inspector.getByRole('textbox', { name: 'Ticket title' });
  await titleEditor.fill('Autosaved inspector title');
  await titleEditor.blur();
  await expect(inspector.getByRole('heading', { name: 'Autosaved inspector title' })).toBeVisible();
  await inspector.getByRole('button', { name: 'Add tag' }).click();
  const tagEditor = page.getByRole('dialog', { name: 'Add tag' }).getByRole('combobox', { name: 'Tag name' });
  await expect(tagEditor).toBeFocused();
  await tagEditor.fill('regression');
  await tagEditor.press('Enter');
  await expect(inspector.locator('[data-component="tag-chip"][data-tag-id="regression"]')).toBeVisible();
  await inspector.locator('[data-component="tag-chip"][data-tag-id="client"]').evaluate(node => node.dispatchEvent(new CustomEvent('wa-remove', { bubbles: true })));
  await expect(inspector.locator('[data-component="tag-chip"][data-tag-id="client"]')).toHaveCount(0);
  await expect(inspector.getByRole('button', { name: 'Info' })).toHaveAttribute('aria-current', 'page');
  await expect(inspector.locator('[data-component="status-badge"]')).toBeVisible();
  await expect(inspector.locator('wa-select[name="inspector-category"] [data-lucide="sparkles"]')).toHaveCount(2);
  await expect(inspector.locator('wa-select[name="inspector-priority"] [data-lucide="chevron-up"]')).toHaveCount(2);
  await expect(inspector.locator('.ticket-category-select .select__icon--selected [data-lucide="sparkles"]')).toBeVisible();
  await expect(inspector.locator('.ticket-priority-select .select__icon--selected [data-lucide="chevron-up"]')).toBeVisible();
  const selectedSpacing = await inspector.locator('wa-select[name="inspector-category"]').evaluate(node => {
    const icon = node.querySelector<HTMLElement>('.select__icon--selected')!.getBoundingClientRect();
    const input = node.shadowRoot!.querySelector<HTMLElement>('[part~="display-input"]')!.getBoundingClientRect();
    return { actual: input.left - icon.right, expected: Number.parseFloat(getComputedStyle(document.documentElement).fontSize) * .5 };
  });
  expect(selectedSpacing.actual).toBeCloseTo(selectedSpacing.expected, 1);
  const category = inspector.locator('wa-select[name="inspector-category"]');
  const selectCaret = await category.evaluate(node => {
    const caret = node.shadowRoot?.querySelector<HTMLElement>('[part~="expand-icon"]');
    return caret ? { transform: getComputedStyle(caret).transform, width: caret.getBoundingClientRect().width } : null;
  });
  expect(selectCaret?.transform).toContain('0.5');
  await category.click();
  const longOption = category.locator('wa-option[value="requirement_change"]');
  const optionLayout = await longOption.evaluate(node => {
    const label = node.shadowRoot?.querySelector<HTMLElement>('[part~="label"]');
    return label ? { whiteSpace: getComputedStyle(label).whiteSpace, optionHeight: node.getBoundingClientRect().height } : null;
  });
  expect(optionLayout?.whiteSpace).toBe('normal');
  expect(optionLayout!.optionHeight).toBeGreaterThan(40);
  for (const value of ['task', 'requirement_change']) {
    const centerDelta = await category.locator(`wa-option[value="${value}"]`).evaluate(node => {
      const label = node.shadowRoot!.querySelector<HTMLElement>('[part~="label"]')!.getBoundingClientRect();
      const start = node.shadowRoot!.querySelector<HTMLElement>('[part~="start"]')!.getBoundingClientRect();
      return Math.abs((label.top + label.height / 2) - (start.top + start.height / 2));
    });
    expect(centerDelta).toBeLessThan(1);
  }
  await page.keyboard.press('Escape');
  await category.evaluate((node: HTMLElement & { value: string }) => { node.value = 'bug'; node.dispatchEvent(new Event('change', { bubbles: true })); });
  await expect(inspector.locator('.ticket-category-select .select__icon--selected [data-lucide="bug"]')).toBeVisible();
  await expect(inspector.locator('.ticket-category-select .select__icon--selected [data-lucide="sparkles"]')).toHaveCount(0);
  const priority = inspector.locator('wa-select[name="inspector-priority"]');
  await priority.evaluate((node: HTMLElement & { value: string }) => { node.value = 'low'; node.dispatchEvent(new Event('change', { bubbles: true })); });
  await expect(inspector.locator('.ticket-priority-select .select__icon--selected [data-lucide="chevron-down"]')).toBeVisible();
  await expect(inspector.locator('.ticket-priority-select .select__icon--selected [data-lucide="chevron-up"]')).toHaveCount(0);
  const star = inspector.getByRole('button', { name: 'Remove from Up Next' });
  await star.click();
  await expect(inspector.getByRole('button', { name: 'Add to Up Next' })).toBeVisible();
  const statusTrigger = inspector.locator('wa-select[name="inspector-status"]');
  await expect(statusTrigger).toHaveAttribute('aria-label', 'Change status, Started');
  await expect(statusTrigger.locator('.select__custom-selected [data-component="status-badge"]')).toHaveAttribute('data-status', 'started');
  await statusTrigger.click();
  await expect(statusTrigger.locator('wa-option [data-lucide]')).toHaveCount(6);
  await expect(statusTrigger.locator('wa-divider')).toHaveCount(1);
  expect(await statusTrigger.locator('wa-option').evaluateAll(options => options.map(option => option.getAttribute('value')))).toEqual(['not_started', 'started', 'completed', 'verified', 'backlog', 'archive']);
  const popupFontWeight = await statusTrigger.locator('wa-option[value="completed"]').evaluate(node => getComputedStyle(node.shadowRoot!.querySelector('[part~="label"]')!).fontWeight);
  expect(Number(popupFontWeight)).toBeLessThanOrEqual(500);
  await statusTrigger.locator('wa-option[value="completed"]').click();
  await expect(inspector.locator('[data-component="status-badge"]')).toHaveAttribute('data-status', 'completed');
  await expect(statusTrigger).toHaveAttribute('aria-label', 'Change status, Completed');
  await expect(statusTrigger.locator('.select__custom-selected [data-component="status-badge"]')).toHaveAttribute('data-status', 'completed');
  await expect(inspector.locator('[data-component="ticket-info-panel"]')).toBeVisible();
  const sectionRhythm = await inspector.locator('[data-component="ticket-info-panel"]').evaluate(node =>
    [...node.querySelectorAll<HTMLElement>('.ticket-inspector__section')].map(section => ({ gap: getComputedStyle(section).rowGap, headerHeight: section.querySelector('header')?.getBoundingClientRect().height })),
  );
  expect(sectionRhythm).toHaveLength(3);
  expect(sectionRhythm.map(section => section.gap)).toEqual(['8.8px', '8.8px', '8.8px']);
  expect(sectionRhythm.map(section => section.headerHeight)).toEqual([undefined, 32, 32]);
  await expect(inspector.getByRole('button', { name: 'Block ticket' })).toBeVisible();
  await inspector.getByRole('button', { name: 'Block ticket' }).click();
  const blockedReason = inspector.getByRole('textbox', { name: 'Blocked reason' });
  await expect(blockedReason).toBeFocused();
  await blockedReason.fill('Waiting for API review.');
  await blockedReason.blur();
  await expect(inspector.locator('[data-component="blocked-badge"]')).toHaveText('Blocked');
  await expect(inspector.getByText('Waiting for API review.')).toBeVisible();
  const blockedSection = inspector.locator('.ticket-inspector__blocked-section');
  const detailsSection = inspector.locator('.ticket-inspector__details-section');
  expect((await blockedSection.boundingBox())!.y).toBeLessThan((await detailsSection.boundingBox())!.y);
  await expect(inspector.locator('[data-component="ticket-notes"] [data-component="note-card"]')).toHaveCount(5);
  const firstNote = inspector.locator('[data-component="ticket-notes"] [data-component="note-card"]').first();
  const firstNoteBody=firstNote.locator('.note-card__body');await expect(firstNoteBody).toHaveAttribute('aria-label', 'Edit note');
  await expect(firstNote.locator('[data-action="open-ticket-reader"]')).toHaveCount(0);
  await firstNoteBody.press('Enter');
  await expect(firstNote.getByRole('textbox', { name: 'Note body' })).toBeFocused();
  await firstNote.getByRole('textbox', { name: 'Note body' }).fill('Autosaved inspector note');
  await firstNote.getByRole('textbox', { name: 'Note body' }).blur();
  await expect(firstNote).toContainText('Autosaved inspector note');
  await inspector.getByRole('button', { name: 'Add note', exact: true }).last().click();
  await expect(page.getByText('Note composer requested')).toBeVisible();
  await inspector.getByRole('button', { name: 'Timeline' }).click();
  await expect(inspector.locator('[data-component="ticket-timeline"]')).toBeVisible();
  await expect(inspector.getByRole('heading', { name: 'Timeline' })).toBeVisible();
  await expect(inspector.locator('.ticket-inspector__timeline > li')).toHaveCount(4);
  await expect(inspector.getByText('4 events total')).toBeVisible();
  await expect(inspector.locator('.ticket-inspector__timeline > li').first()).toContainText('Claude started work');
  await inspector.getByRole('button', { name: 'Code Review' }).click();
  await expect(inspector.locator('[data-component="ticket-code-review"] .ticket-code-review__commit')).toHaveCount(2);
  await inspector.getByRole('button', { name: 'Open 2 commit bundle 92ed71a through c4a38be in Glassbox' }).click();
  await expect(page.getByText('Commit range opened in Glassbox')).toBeVisible();
  await page.screenshot({ path: '/private/tmp/hs2-pg1hkj-code-review-wide.png', fullPage: true });
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(inspector.locator('[data-component="ticket-code-review"]')).toBeVisible();
  await expect(inspector.locator('[data-commit-sha]').first().locator('strong')).toHaveCSS('overflow-wrap', 'anywhere');
  await page.screenshot({ path: '/private/tmp/hs2-pg1hkj-code-review-narrow.png', fullPage: true });
  await page.setViewportSize({ width: 1280, height: 720 });
  await inspector.getByRole('button', { name: 'Attachments' }).click();
  await expect(inspector.locator('[data-component="ticket-attachments"]')).toBeVisible();
  await expect(inspector.getByRole('heading', { name: 'Attachments' })).toBeVisible();
  await expect(inspector.locator('[data-attachment-id]')).toHaveCount(2);
  await expect(inspector.getByText('2 attachments total')).toBeVisible();
  await expect(inspector.locator('[data-action="toggle-inspector-up-next"]')).toHaveCount(0);
  await inspector.getByRole('button', { name: 'Hide inspector' }).click();
  await expect(inspector).toHaveCount(0);
  await page.getByRole('button', { name: 'Open ticket inspector' }).click();
  const reopened = page.locator('[data-component="ticket-inspector"]');
  await expect(reopened).toBeVisible();
  await reopened.getByRole('button', { name: 'Info' }).click();
  await expect(reopened.locator('.ticket-category-select .select__icon--selected [data-lucide="bug"]')).toBeVisible();
  await expect(reopened.locator('.ticket-priority-select .select__icon--selected [data-lucide="chevron-down"]')).toBeVisible();
  await expect(reopened.locator('[data-component="status-badge"]')).toHaveAttribute('data-status', 'completed');
  await expect(reopened.getByRole('button', { name: 'Open ticket reader', exact: true })).toBeVisible();
  await reopened.getByRole('button', { name: 'Open ticket reader', exact: true }).click();
  await expect(page).toHaveURL('/ux-demo?component=ticket-reader');
  await expect(page.locator('[data-component="ticket-reader"]')).toBeVisible();
});

test('renders standalone ticket metadata and inspector-section demos', async ({ page }) => {
  for (const [id, component] of [['ticket-category-select', 'ticket-category-select'], ['ticket-priority-select', 'ticket-priority-select'], ['ticket-status-menu', 'ticket-status-menu'], ['ticket-info-panel', 'ticket-info-panel'], ['ticket-timeline', 'ticket-timeline'], ['ticket-code-review', 'ticket-code-review'],['attachment-gallery','attachment-gallery'], ['ticket-attachments', 'ticket-attachments']] as const) {
    await page.goto(`/ux-demo?component=${id}`);
    await expect(page.locator(`[data-component="${component}"]`).or(page.locator(`.${component}`)).first()).toBeVisible();
  }
  await expect(page.getByRole('button',{name:'Edit batch label Brian · Round 1 · Problem evidence'})).toBeVisible();await expect(page.getByRole('button',{name:'Edit batch label Corrected implementation'})).toBeVisible();await expect(page.getByRole('button',{name:'Edit batch label System · Batch 1 · Problem evidence'})).toBeVisible();await expect(page.getByRole('button',{name:'Edit batch label Legacy / Uncategorized'})).toBeVisible();
  const annotatedCard=page.getByRole('button',{name:'Open wide-layout.svg in media gallery, 2 annotations'});await expect(annotatedCard.locator('.ticket-attachments__annotation-marker [data-lucide="pencil"]')).toBeVisible();
  await page.locator('.demo-detail').screenshot({ path: '/private/tmp/hs2-6fp1kt-attachment-batches-final-wide.png' });
  const surface=page.locator('[data-component="ticket-attachments"]'),dragged=page.locator('[data-drag-attachment-id="wide"]');await dragged.evaluate(node=>node.dispatchEvent(new DragEvent('dragstart',{bubbles:true,dataTransfer:new DataTransfer()})));await surface.evaluate(node=>(node as HTMLElement).dataset.draggingGroupAttachment='true');const newGroup=page.locator('[data-attachment-new-group-drop-target]');await expect(newGroup).toBeVisible();await expect(page.locator('input[type="checkbox"]')).toHaveCount(0);await surface.screenshot({path:'/private/tmp/hs2-c0r4mx-drag-new-group.png'});await newGroup.evaluate(node=>node.dispatchEvent(new DragEvent('drop',{bubbles:true,cancelable:true,dataTransfer:new DataTransfer()})));await expect(page.locator('[data-attachment-group-drop-target]')).toHaveCount(5);await expect(page.getByRole('button',{name:'Edit batch label New group'})).toBeVisible();await surface.screenshot({path:'/private/tmp/hs2-c0r4mx-regrouped-final.png'});
  const item=page.locator('[data-attachment-id="demo-video"]'),trigger=item.getByRole('button',{name:'More actions for choppy.mov'});await expect(item.getByRole('button')).toHaveCount(1);await expect(trigger).toHaveAttribute('title','More actions for choppy.mov');await expect(trigger.locator('[data-lucide="more-horizontal"]')).toBeVisible();await trigger.click();let menu=page.getByRole('menu',{name:'Attachment actions'});await expect(menu.locator('[data-component="menu-item"]')).toHaveCount(6);await expect(menu.getByRole('menuitem').allTextContents()).resolves.toEqual(['Open','Download','Copy reference','Rename','Show in file manager','Remove']);
  const gridVideo=page.locator('.ticket-attachments__image-grid video').first();await expect(gridVideo).toBeVisible();await expect(gridVideo).toHaveAttribute('poster','/ux-gallery-preview.svg?variant=video');expect(await gridVideo.evaluate(node=>(node as HTMLVideoElement).paused)).toBe(true);expect(await gridVideo.evaluate(node=>(node as HTMLVideoElement).autoplay)).toBe(false);
  await menu.getByRole('menuitem',{name:'Copy reference'}).click();await expect(menu).toHaveCount(0);await page.locator('[data-component="ticket-attachments"]').screenshot({path:'/private/tmp/hs2-j978e9-annotation-grid-marker.png'});
  await page.setViewportSize({ width: 390, height: 844 });
  const batchHeader=page.locator('.ticket-attachments__batch > header').first(),batchTitle=batchHeader.locator('.ticket-attachments__batch-title'),batchPurpose=batchHeader.locator('select');expect((await batchPurpose.boundingBox())!.y).toBeGreaterThan((await batchTitle.boundingBox())!.y);await item.scrollIntoViewIfNeeded();await item.click({button:'right'});menu=page.getByRole('menu',{name:'Attachment actions'});await expect(menu).toBeVisible();await expect(menu.locator('[data-component="menu-item"]')).toHaveCount(6);await page.screenshot({ path: '/private/tmp/hs2-6fp1kt-attachment-batches-final-narrow.png',fullPage:true });
});

test('styles and edits attachment group labels while preserving drag regrouping', async ({page}) => {
  await page.setViewportSize({width:1280,height:900});
  await page.goto('/ux-demo?component=ticket-attachments');
  const surface=page.locator('[data-component="ticket-attachments"]'),groups=surface.locator('[data-attachment-group-drop-target]'),first=groups.first();
  await expect(first).toHaveCSS('background-color','rgba(0, 0, 0, 0)');
  await expect(surface.locator('[data-lucide="grip-vertical"]')).toHaveCount(0);
  const title=first.getByRole('button',{name:/Edit batch label/}),purpose=first.locator('select');
  await expect(title).toHaveCSS('font-size','16px');
  expect((await purpose.boundingBox())!.width).toBeLessThan((await first.locator('header').boundingBox())!.width);
  await title.dblclick();
  const editor=first.getByRole('textbox',{name:/Batch label/});
  await expect(editor).toBeFocused();
  await editor.fill('Discarded label');
  await editor.press('Escape');
  await expect(first.getByRole('button',{name:/Brian · Round 1 · Problem evidence/})).toBeVisible();
  await expect(first.getByRole('button',{name:/Brian · Round 1 · Problem evidence/})).toBeFocused();
  await first.getByRole('button',{name:/Edit batch label/}).dblclick();
  await editor.fill('Human review evidence');
  await editor.press('Enter');
  await expect(first.getByRole('button',{name:'Edit batch label Human review evidence'})).toBeVisible();
  await expect(first.getByRole('button',{name:'Edit batch label Human review evidence'})).toBeFocused();
  await page.locator('h2').first().click();
  await page.evaluate(() => new Promise<void>(resolve => requestAnimationFrame(() => { resolve(); })));
  await surface.evaluate(node=>(node as HTMLElement).dataset.draggingGroupAttachment='true');
  const activeTarget=groups.nth(1);
  await activeTarget.evaluate(node=>(node as HTMLElement).dataset.dragOver='true');
  await expect(activeTarget).toHaveAttribute('data-drag-over','true');
  await expect(activeTarget).not.toHaveCSS('outline-style','none');
  await expect(surface.locator('[data-attachment-new-group-drop-target]')).toBeVisible();
  await surface.screenshot({path:'/private/tmp/hs2-c0r4mx-feedback-wide.png'});
  await page.setViewportSize({width:390,height:844});
  await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
  await page.mouse.move(2,2);
  await expect(first.getByRole('button',{name:'Edit batch label Human review evidence'})).toBeVisible();
  expect((await purpose.boundingBox())!.width).toBeLessThan((await first.locator('header').boundingBox())!.width);
  await surface.screenshot({path:'/private/tmp/hs2-c0r4mx-feedback-narrow.png'});
});

test('navigates and zooms the standalone attachment gallery demo',async({page})=>{
  await page.route('**/ux-gallery-preview.svg',async route=>{if(new URL(route.request().url()).search){await route.fallback();return}await route.fulfill({contentType:'image/svg+xml',body:'<svg xmlns="http://www.w3.org/2000/svg" width="1200" height="700"><rect width="1200" height="700" fill="#ffe0a8"/><path d="M0 180V0h180M1020 0h180v180M1200 520v180h-180M180 700H0V520" fill="#c64b2c"/><text x="600" y="375" text-anchor="middle" font-family="sans-serif" font-size="58" fill="#33200f">Source corners preserved</text></svg>'})});await page.setViewportSize({width:1280,height:900});await page.goto('/ux-demo?component=attachment-gallery');let gallery=page.locator('[data-component="attachment-gallery"]');await expect(gallery).toHaveAttribute('aria-label',/Image 1 of 3/);await expect(gallery.getByRole('button',{name:'Annotate media, 1 annotation'}).locator('.attachment-gallery__annotation-count')).toHaveText('1');const mediaWrap=gallery.locator('.attachment-gallery__media-wrap'),image=gallery.locator('[data-gallery-image="true"]');await expect(mediaWrap).toHaveCSS('border-radius','0px');await expect(image).toHaveCSS('border-radius','0px');await gallery.screenshot({path:'/private/tmp/hs2-2fm7ed-square-media-wide.png'});await expect(gallery.locator('[data-component="toolbar"]')).toBeVisible();const darkGroups=gallery.locator('[data-component="toolbar-control-group"]');await expect(darkGroups).toHaveCount(4);for(const group of await darkGroups.all()){await expect(group).toHaveAttribute('data-tone','dark');await expect(group).toHaveCSS('color','rgb(255, 255, 255)');await expect(group).not.toHaveCSS('border-color','rgb(209, 209, 214)')}await expect(gallery.locator('.attachment-gallery__filename')).toHaveCSS('color','rgb(255, 255, 255)');const fitWidth=await image.evaluate(node=>(node as HTMLElement).getBoundingClientRect().width);await gallery.getByRole('button',{name:'Zoom in'}).click();expect(await image.evaluate(node=>(node as HTMLElement).getBoundingClientRect().width)).toBeGreaterThan(fitWidth);await page.setViewportSize({width:760,height:640});await expect(mediaWrap).toHaveCSS('border-radius','0px');await expect(image).toHaveCSS('border-radius','0px');await gallery.screenshot({path:'/private/tmp/hs2-2fm7ed-square-media-narrow.png'});await page.setViewportSize({width:1280,height:900});await gallery.getByRole('button',{name:'Next image'}).click();await expect(gallery).toHaveAttribute('aria-label',/Image 2 of 3/);await gallery.screenshot({path:'/private/tmp/hs2-ddpkts-gallery-dark-wide.png'});await gallery.locator('[data-component="toolbar"]').screenshot({path:'/private/tmp/hs2-ddpkts-gallery-dark-toolbar.png'});await page.setViewportSize({width:760,height:640});await gallery.getByRole('button',{name:'Next image'}).click();await expect(gallery).toHaveAttribute('aria-label',/Video 3 of 3: walkthrough.mp4/);const video=gallery.locator('video');await expect(video).not.toHaveAttribute('controls',/.+/);await expect(gallery.getByRole('button',{name:'Play'})).toBeVisible();await expect(gallery.getByRole('button',{name:'Volume controls'})).toBeVisible();await expect(video).toHaveCSS('border-radius','0px');expect(await video.evaluate(node=>(node as HTMLVideoElement).paused)).toBe(true);await video.evaluate(async node=>{if(!(node instanceof HTMLVideoElement))throw new Error('Expected gallery video');const canvas=document.createElement('canvas');canvas.width=640;canvas.height=360;const context=canvas.getContext('2d')!;context.fillStyle='#7857a4';context.fillRect(0,0,640,360);context.fillStyle='white';context.font='32px sans-serif';context.fillText('Hot Sheet video evidence',132,190);const recorder=new MediaRecorder(canvas.captureStream(5),{mimeType:'video/webm'}),chunks:Blob[]=[];recorder.ondataavailable=event=>chunks.push(event.data);recorder.start();await new Promise(resolve=>setTimeout(resolve,250));const stopped=new Promise(resolve=>{recorder.onstop=resolve});recorder.stop();await stopped;node.src=URL.createObjectURL(new Blob(chunks,{type:'video/webm'}));await new Promise((resolve,reject)=>{node.onloadedmetadata=resolve;node.onerror=reject})});await expect(video).toHaveJSProperty('videoWidth',640);const stageBox=(await gallery.locator('.attachment-gallery__stage').boundingBox())!,footerBox=(await gallery.locator('.attachment-gallery__footer').boundingBox())!;expect(stageBox.y+stageBox.height).toBeLessThanOrEqual(footerBox.y+.5);await video.hover();await gallery.screenshot({path:'/private/tmp/hs2-hz0trg-video-gallery-narrow.png'});await gallery.getByRole('button',{name:'Close video gallery'}).click();await expect(gallery).toHaveCount(0);await page.getByRole('button',{name:'Open gallery'}).click();gallery=page.locator('[data-component="attachment-gallery"]');await expect(gallery).toBeVisible();
});

test('deselects an image annotation when the media canvas is clicked away',async({page})=>{
  await page.goto('/ux-demo?component=attachment-gallery');const gallery=page.locator('[data-component="attachment-gallery"]');await gallery.getByRole('button',{name:'Annotate media, 1 annotation'}).click();const annotation=gallery.getByRole('button',{name:/Review this alignment/});await expect(annotation).toHaveAttribute('data-selected','true');await expect(annotation.locator('[data-annotation-handle]')).toHaveCount(8);const surface=gallery.locator('[data-gallery-annotation-surface="true"]'),box=(await surface.boundingBox())!;await surface.click({position:{x:box.width*.9,y:box.height*.9}});await expect(annotation).toHaveAttribute('data-selected','false');await expect(annotation.locator('[data-annotation-handle]')).toHaveCount(0);
});

test('previews and manipulates custom video and annotation timeline controls',async({page})=>{
  await page.setViewportSize({width:1280,height:900});await page.goto('/ux-demo?component=attachment-gallery');const gallery=page.locator('[data-component="attachment-gallery"]');await gallery.getByRole('button',{name:'Next image'}).click();await gallery.getByRole('button',{name:'Next image'}).click();await expect(gallery).toHaveAccessibleName(/Video 3 of 3/);await expect(gallery.locator('video')).not.toHaveAttribute('controls',/.+/);const volumeButton=gallery.getByRole('button',{name:'Volume controls'});await volumeButton.click();await expect(volumeButton).toHaveAttribute('aria-expanded','true');await gallery.getByRole('button',{name:'Mute video'}).click();await expect(gallery.getByRole('button',{name:'Unmute video'})).toBeVisible();const volume=gallery.getByRole('slider',{name:'Video volume'});await volume.fill('0.4');await expect(volume).toHaveValue('0.4');await gallery.screenshot({path:'/private/tmp/hs2-35n9rs-volume-popup-wide.png'});await gallery.locator('.attachment-gallery__filename').click();await expect(volumeButton).toHaveAttribute('aria-expanded','false');await volumeButton.click();await expect(volume).toBeVisible();await gallery.getByRole('button',{name:'Annotate media'}).click();await expect(gallery.getByRole('slider',{name:'Video position'})).toBeVisible();const start=gallery.getByRole('button',{name:/Annotation range start/}),end=gallery.getByRole('button',{name:/Annotation range end/});await expect(start).toBeVisible();await expect(end).toBeVisible();await start.focus();await page.keyboard.press('ArrowRight');await expect(gallery.getByRole('button',{name:'Annotation range start at 0:01'})).toBeVisible();const track=(await gallery.locator('.attachment-gallery__timeline-track').boundingBox())!;await end.hover();await page.mouse.down();await page.mouse.move(track.x+track.width*.75,track.y+track.height/2);await page.mouse.up();await expect(gallery.getByRole('button',{name:'Annotation range end at 0:04'})).toBeVisible();await expect(gallery.getByText('Transition is abrupt')).toBeVisible();await gallery.screenshot({path:'/private/tmp/hs2-hz0trg-video-annotations-wide.png'});await page.setViewportSize({width:760,height:640});await volumeButton.click();await gallery.screenshot({path:'/private/tmp/hs2-35n9rs-volume-popup-narrow.png'});await gallery.getByRole('button',{name:'Close video gallery'}).click();await expect(gallery).toHaveCount(0);await page.getByRole('button',{name:'Open gallery'}).click();await expect(page.locator('[data-component="attachment-gallery"]')).toBeVisible();
});

test('represents animated SVG evidence with the shared timed annotation controls',async({page})=>{
  await page.route('**/ux-gallery-preview.svg?variant=narrow',route=>route.fulfill({contentType:'image/svg+xml',body:'<svg xmlns="http://www.w3.org/2000/svg" width="640" height="360"><rect width="640" height="360"><animate attributeName="fill" values="#333;#777;#333" dur="4s" repeatCount="indefinite"/></rect></svg>'}));await page.goto('/ux-demo?component=attachment-gallery');const gallery=page.locator('[data-component="attachment-gallery"]');await gallery.getByRole('button',{name:'Next image'}).click();await expect(gallery).toHaveAccessibleName(/Image 2 of 3/);await expect(gallery.getByRole('slider',{name:'Video position'})).toBeVisible();await gallery.getByRole('button',{name:'Annotate media'}).click();await expect(gallery.getByRole('button',{name:/Annotation range start/})).toBeVisible();await gallery.screenshot({path:'/private/tmp/hs2-jw17a4-animated-svg-annotations.png'});
});

test('opens the shared TicketReader intent when a composed row is double-clicked', async ({ page }) => {
  await page.goto('/ux-demo?component=ticket-list');
  await page.locator('[data-component="ticket-list-row"]').first().dblclick();
  await expect(page.getByRole('heading', { name: 'TicketReader', exact: true })).toBeVisible();
});

test('adjusts and removes TagChip through its settings inspector', async ({ page }) => {
  await page.setViewportSize({ width: 1600, height: 900 });
  await page.goto('/ux-demo?component=tag-chip');
  const chip = page.locator('[data-component="tag-chip"]');
  await expect(chip).toContainText('needs-design');
  const padding = await chip.evaluate(node => {
    const style = getComputedStyle(node);
    return { horizontal: Number.parseFloat(style.paddingLeft), vertical: Number.parseFloat(style.paddingTop) };
  });
  expect(padding.horizontal / padding.vertical).toBeCloseTo(2, 1);
  const toggle = page.locator('[data-action="toggle-settings"]');
  await expect(toggle).toHaveCount(1);
  await expect(toggle).toContainText('Settings');
  const closedToggleBox = await toggle.boundingBox();
  await toggle.click();
  await expect(toggle).toHaveCount(1);
  await expect(toggle).toContainText('Close settings');
  await expect(toggle).toHaveAttribute('aria-expanded', 'true');
  const openToggleBox = await toggle.boundingBox();
  expect(closedToggleBox).not.toBeNull();
  expect(openToggleBox).not.toBeNull();
  expect(Math.abs(openToggleBox!.x - closedToggleBox!.x)).toBeLessThanOrEqual(1);
  expect(Math.abs(openToggleBox!.y - closedToggleBox!.y)).toBeLessThanOrEqual(1);
  const inspector = page.getByRole('complementary', { name: 'TagChip settings' });
  await expect(inspector).toBeVisible();
  const [detailBox, inspectorBox] = await Promise.all([
    page.locator('.demo-detail').boundingBox(),
    inspector.boundingBox(),
  ]);
  expect(detailBox).not.toBeNull();
  expect(inspectorBox).not.toBeNull();
  expect(detailBox!.x + detailBox!.width).toBeLessThanOrEqual(inspectorBox!.x + 1);
  const label = page.getByRole('textbox', { name: 'Label' });
  await label.fill('server');
  await expect(chip).toContainText('server');
  await expect(inspector).toBeVisible();
  await page.locator('wa-select[name="variant"]').evaluate((node: HTMLElement & { value: string }) => {
    node.value = 'success'; node.dispatchEvent(new Event('change', { bubbles: true }));
  });
  await expect(chip).toHaveAttribute('variant', 'success');
  await expect(inspector).toBeVisible();
  await inspector.locator('wa-checkbox[name="pill"]').click();
  await expect(inspector).toBeVisible();
  await chip.locator('[part~="remove-button"]').click();
  await expect(page.getByText('Remove requested for demo-tag')).toBeVisible();
  await expect(inspector).toBeVisible();
  await inspector.getByRole('button', { name: 'Reset' }).click();
  await expect(label).toHaveJSProperty('value', 'needs-design');
  await expect(page.locator('wa-select[name="variant"]')).toHaveJSProperty('value', 'neutral');
  await expect(page.locator('wa-select[name="appearance"]')).toHaveJSProperty('value', 'filled');
  await expect(page.locator('wa-select[name="size"]')).toHaveJSProperty('value', 'small');
  await expect(inspector.locator('wa-checkbox[name="removable"]')).toHaveJSProperty('checked', true);
  await expect(inspector.locator('wa-checkbox[name="pill"]')).toHaveJSProperty('checked', false);
  await expect(inspector.locator('wa-checkbox[name="disabled"]')).toHaveJSProperty('checked', false);
  await expect(chip).toContainText('needs-design');
  await expect(chip).toHaveAttribute('variant', 'neutral');
  await expect(page.getByText('No actions yet')).toBeVisible();
  await expect(inspector).toBeVisible();
  await toggle.click();
  await expect(inspector).toBeHidden();
  await expect(toggle).toContainText('Settings');
  await expect(toggle).toHaveAttribute('aria-expanded', 'false');
});

test('uses semantic cursors across native and Web Awesome interactions', async ({ page }) => {
  await page.goto('/ux-demo?component=ticket-row');
  await expect(page.locator('[data-component="ticket-list-row"]')).toHaveCSS('cursor', 'pointer');
  await expect(page.locator('[data-action="toggle-row-up-next"]')).toHaveCSS('cursor', 'pointer');
  await page.locator('[data-action="toggle-settings"]').click();
  const inspector = page.getByRole('complementary', { name: 'TicketRow settings' });
  await expect(inspector.getByRole('textbox', { name: 'Title' })).toHaveCSS('cursor', 'text');
  const selectCursor = await inspector.locator('wa-select[name="status"]').evaluate(node => getComputedStyle(node.shadowRoot!.querySelector('[part~="combobox"]')!).cursor);
  expect(selectCursor).toBe('pointer');
  const checkboxCursor = await inspector.locator('wa-checkbox[name="up-next"]').evaluate(node => getComputedStyle(node.shadowRoot!.querySelector('[part~="checkbox"]')!).cursor);
  expect(checkboxCursor).toBe('pointer');
  const buttonCursor = await inspector.locator('wa-button[data-action="reset-settings"]').evaluate(node => getComputedStyle(node.shadowRoot!.querySelector('[part~="base"]')!).cursor);
  expect(buttonCursor).toBe('pointer');
  await page.locator('[data-action="toggle-settings"]').click();
  await page.locator('[data-component="ticket-list-row"]').click({ button: 'right' });
  await expect(page.getByRole('menu', { name: 'Ticket actions' }).locator('wa-dropdown-item').first()).toHaveCSS('cursor', 'pointer');
  await page.goto('/ux-demo?component=quick-ticket-composer');
  await page.getByRole('button', { name: /New ticket/ }).click();
  const create = page.locator('wa-button[type="submit"]');
  await create.evaluate(node => { node.setAttribute('disabled', ''); });
  await expect.poll(() => create.evaluate(node => getComputedStyle(node.shadowRoot!.querySelector('[part~="base"]')!).cursor)).toBe('not-allowed');
});

test('exercises the five ProjectSidebar component demos and their controlled transitions', async ({ page }) => {
  await page.goto('/ux-demo?component=project-summary');
  const summaries = page.locator('[data-component="project-summary"]');
  await expect(summaries).toHaveCount(2);
  const summary = page.locator('[data-component="project-summary"][data-chart-tone="brand"]');
  const aggregateSummary = page.locator('[data-component="project-summary"][data-chart-tone="success"]');
  await expect(aggregateSummary.locator('[data-bar="0"]')).toHaveCSS('background-color','rgb(52, 199, 89)');
  await expect(summary).toContainText('6 completed today');
  await expect(summary).toContainText('3 in progress');
  await expect(summary).not.toContainText('%');
  await expect(summary.locator('[data-bar]')).toHaveCount(7);
  await expect(summary.locator('[data-zero="true"]')).toHaveCount(1);
  await expect(summary.locator('[data-zero="true"]')).toHaveCSS('height', '1px');
  await expect(summary.locator('[data-zero="true"]')).toHaveCSS('background-color', 'rgb(174, 174, 178)');
  await expect(summary).toHaveCSS('cursor','pointer');
  const summaryGeometry=await summary.evaluate(node=>{const button=node.getBoundingClientRect(),chart=node.querySelector('.project-summary__chart')!.getBoundingClientRect(),counts=node.querySelector('.project-summary__counts')!.getBoundingClientRect();return{button:{top:button.top,bottom:button.bottom},contentTop:Math.min(chart.top,counts.top),contentBottom:Math.max(chart.bottom,counts.bottom)}});
  expect(summaryGeometry.button.top).toBeLessThanOrEqual(summaryGeometry.contentTop);expect(summaryGeometry.button.bottom).toBeGreaterThanOrEqual(summaryGeometry.contentBottom);
  await summary.hover();await page.screenshot({path:'/private/tmp/hs2-j5c5xg-project-summary-hit-area.png',fullPage:true});
  await summary.click();
  await expect(page.getByText('Hot Sheet 2 project statistics requested.')).toBeVisible();

  await page.goto('/ux-demo?component=repository-summary');
  const repository = page.getByRole('button', { name: 'Repository status for feature/client-sidebar' });
  await expect(repository.locator('[data-lucide="git-branch"]')).toHaveCount(1);
  await expect(repository.locator('.repository-summary__branch-name')).toHaveCSS('direction', 'rtl');
  await expect(repository.locator('.repository-summary__branch-name')).toHaveCSS('text-overflow', 'ellipsis');
  await repository.click();
  await expect(page.getByText('Repository status requested.')).toBeVisible();

  await page.goto('/ux-demo?component=view-navigation');
  const views = page.locator('[data-component="view-navigation"]');
  const navigationGeometry = await views.evaluate(node => {
    const header = node.querySelector('header')!.getBoundingClientRect();
    const button = node.querySelector('li button')!.getBoundingClientRect();
    const icon = node.querySelector('li button svg')!.getBoundingClientRect();
    return { headerLeft: header.left, buttonLeft: button.left, iconLeft: icon.left, headerRight: header.right, buttonRight: button.right };
  });
  expect(navigationGeometry.headerLeft).toBeCloseTo(navigationGeometry.buttonLeft, 0);
  expect(navigationGeometry.buttonRight).toBeCloseTo(navigationGeometry.headerRight, 0);
  expect(navigationGeometry.iconLeft - navigationGeometry.buttonLeft).toBeGreaterThan(8);
  expect(navigationGeometry.iconLeft - navigationGeometry.buttonLeft).toBeLessThan(11);
  await expect(views.getByRole('button', { name: /Queue/ })).toHaveAttribute('aria-current', 'page');
  await views.getByRole('button', { name: /Needs Review/ }).click();
  await expect(views.getByRole('button', { name: /Needs Review/ })).toHaveAttribute('aria-current', 'page');
  await expect(views.getByRole('button', { name: /Queue/ })).not.toHaveAttribute('aria-current', 'page');
  const addView = views.getByRole('button', { name: 'Add view' });
  await expect(addView).toBeDisabled();
  await expect(addView).toHaveAttribute('title', 'Custom views are not available yet.');

  await page.goto('/ux-demo?component=command-navigation');
  const commands = page.locator('[data-component="command-navigation"]');
  const heading = commands.getByRole('button', { name: /Project commands/ });
  await expect(heading).toHaveAttribute('aria-expanded', 'true');
  await expect(commands.getByRole('button', { name: 'Verify project' })).toHaveCSS('background-color', 'rgb(20, 184, 166)');
  await expect(commands.getByRole('button', { name: 'Build clients' })).toHaveCSS('background-color', 'rgb(249, 115, 22)');
  await expect(commands.getByRole('button', { name: 'Publish preview' })).toHaveCSS('background-color', 'rgb(139, 92, 246)');
  await commands.getByRole('button', { name: 'Verify project' }).click();
  await expect(commands.getByRole('button', { name: /Running Verify project/ })).toHaveAttribute('aria-pressed', 'true');
  await commands.getByRole('button', { name: /Running Verify project/ }).click();
  await expect(commands.getByRole('button', { name: 'Verify project' })).toHaveAttribute('aria-pressed', 'false');
  await heading.click();
  await expect(commands.getByRole('button', { name: 'Verify project' })).toHaveCount(0);
  await expect(heading).toHaveAttribute('aria-expanded', 'false');

  await page.goto('/ux-demo?component=drive-control');
  const drive = page.locator('[data-component="drive-control"]'),driveAction=drive.locator('[data-action="toggle-drive"]');
  await expect(driveAction).toHaveAccessibleName('Drive with Codex');
  await drive.click();
  await expect(driveAction).toHaveAccessibleName('Codex workflow is running');
  await drive.click();
  await expect(driveAction).toHaveAccessibleName('Drive with Codex');
});

test('holds the AppShell at its 1024 by 600 supported floor',async({page})=>{
  await page.setViewportSize({width:800,height:500});await page.goto('/ux-demo?component=app-shell');const shell=page.locator('[data-component="app-shell"]');const bounds=await shell.boundingBox();expect(bounds?.width).toBeGreaterThanOrEqual(1024);expect(bounds?.height).toBeGreaterThanOrEqual(600);await expect(shell.locator('[data-component="resizable-region"][data-region-id="app-sidebar"]')).toBeVisible();await expect(shell.locator('[data-component="resizable-region"][data-region-id="app-inspector"]')).toBeVisible();await page.screenshot({path:'/private/tmp/hs2-501eph-shell-floor.png',fullPage:true});
});

test('keeps workspace spacing and the new-ticket action in the page header',async({page})=>{
  await page.setViewportSize({width:1728,height:971});await page.goto('/ux-demo?component=app-shell');const shell=page.locator('[data-component="app-shell"]'),workArea=shell.locator('.app-shell__work-area'),workspace=shell.locator('.app-shell__workspace'),header=shell.locator('[data-component="page-header"]');await expect(workArea).toHaveAttribute('data-has-composer','false');
  expect(await shell.evaluate(node=>{const workspace=node.querySelector<HTMLElement>('.app-shell__workspace')!,header=node.querySelector<HTMLElement>('[data-component="page-header"]')!,title=header.querySelector('h1')!,launcher=header.querySelector<HTMLElement>('[data-component="quick-ticket-composer-launcher"]')!,tabs=node.querySelector<HTMLElement>('.ticket-inspector__tabs')!,content=node.querySelector<HTMLElement>('.ticket-inspector__content')!;return{workspacePaddingTop:getComputedStyle(workspace).paddingTop,titleCenter:title.getBoundingClientRect().y+title.getBoundingClientRect().height/2,launcherCenter:launcher.getBoundingClientRect().y+launcher.getBoundingClientRect().height/2,tabsMarginBottom:getComputedStyle(tabs).marginBottom,contentPaddingTop:getComputedStyle(content).paddingTop,tabsToContent:content.getBoundingClientRect().top-tabs.getBoundingClientRect().bottom}})).toEqual({workspacePaddingTop:'16px',titleCenter:expect.any(Number),launcherCenter:expect.any(Number),tabsMarginBottom:'16px',contentPaddingTop:'0px',tabsToContent:16});
  const centers=await header.evaluate(node=>{const title=node.querySelector('h1')!.getBoundingClientRect(),button=node.querySelector('button')!.getBoundingClientRect();return Math.abs(title.y+title.height/2-(button.y+button.height/2))});expect(centers).toBeLessThan(1);
  await shell.getByRole('button',{name:'Columns view'}).click();await expect(workspace).toHaveAttribute('data-presentation','edge-to-edge');expect(await workspace.evaluate(node=>({paddingTop:getComputedStyle(node).paddingTop,boardTop:node.querySelector('.ticket-board')!.getBoundingClientRect().top-node.getBoundingClientRect().top}))).toEqual({paddingTop:'16px',boardTop:16});await page.screenshot({path:'/private/tmp/hs2-f943hj-owned-spacing-wide.png',fullPage:true});
  await shell.getByRole('button',{name:'Settings view'}).click();await expect(header.getByRole('button',{name:/New ticket/})).toHaveCount(0);await expect(workspace).toHaveCSS('padding-top','16px');await shell.getByRole('button',{name:'List view'}).click();await expect(header.getByRole('button',{name:/New ticket/})).toHaveCount(1);
  await page.setViewportSize({width:1024,height:600});await page.locator('.demo-master,.demo-detail__header,.demo-detail__footer').evaluateAll(nodes=>{nodes.forEach(node=>{(node as HTMLElement).style.display='none'})});await page.locator('.demo-shell').evaluate(node=>{(node as HTMLElement).style.gridTemplateColumns='1fr'});await page.locator('.demo-detail,.component-stage').evaluateAll(nodes=>{nodes.forEach(node=>{(node as HTMLElement).style.padding='0';(node as HTMLElement).style.border='0'})});await expect(workspace).toHaveCSS('padding-top','16px');await page.screenshot({path:'/private/tmp/hs2-f943hj-owned-spacing-narrow.png',fullPage:true});
});

test('composes and operates the complete ProjectSidebar demo', async ({ page }) => {
  await page.goto('/ux-demo?component=project-sidebar');
  const sidebar = page.locator('[data-component="project-sidebar"]');
  await expect(sidebar).toBeVisible();
  await expect(sidebar.locator('[data-component="project-work-summary"]')).toHaveText('17 open, 4 up next, 2 active');
  for (const component of ['project-summary', 'repository-summary', 'view-navigation', 'command-navigation', 'drive-control']) await expect(sidebar.locator(`[data-component="${component}"]`)).toHaveCount(1);
  const menuHeaderLefts = await sidebar.locator('[data-component="menu-header"]').evaluateAll(headers => headers.map(header => header.querySelector('h2, span')!.getBoundingClientRect().left));
  expect(menuHeaderLefts).toHaveLength(2);
  expect(menuHeaderLefts[0]).toBeCloseTo(menuHeaderLefts[1], 0);
  const viewActionAlignment = await sidebar.evaluate(node => {
    const action = node.querySelector<HTMLElement>('.view-navigation [data-component="menu-header"] button')!.getBoundingClientRect();
    const item = node.querySelector<HTMLElement>('.view-navigation .menu-item')!.getBoundingClientRect();
    return { actionRight: action.right, itemRight: item.right };
  });
  expect(viewActionAlignment.actionRight).toBeCloseTo(viewActionAlignment.itemRight, 0);
  const alignedRows = await sidebar.evaluate(node => ['.repository-summary .menu-item', '.view-navigation .menu-item', '.command-navigation .menu-item'].map(selector => node.querySelector(selector)!).map(item => {
    const bounds = item.getBoundingClientRect();
    const icon = item.querySelector('.menu-item__icon')!.getBoundingClientRect();
    const label = item.querySelector('.menu-item__label')!.getBoundingClientRect();
    return { left: bounds.left, right: bounds.right, icon: icon.left, label: label.left };
  }));
  expect(alignedRows).toHaveLength(3);
  for (const row of alignedRows.slice(1)) {
    expect(row.left).toBeCloseTo(alignedRows[0].left, 0);
    expect(row.right).toBeCloseTo(alignedRows[0].right, 0);
    expect(row.icon).toBeCloseTo(alignedRows[0].icon, 0);
    expect(row.label).toBeCloseTo(alignedRows[0].label, 0);
  }
  const command = sidebar.getByRole('button', { name: 'Verify project' });
  const commandColors = await command.evaluate(node => ({ color: getComputedStyle(node).color, background: getComputedStyle(node).backgroundColor }));
  await command.hover();
  await expect(command).toHaveCSS('color', commandColors.color);
  await expect(command).toHaveCSS('background-color', commandColors.background);
  await sidebar.getByRole('button', { name: /Backlog/ }).click();
  await expect(sidebar.getByRole('button', { name: /Backlog/ })).toHaveAttribute('aria-current', 'page');
  await command.click();
  await expect(sidebar.getByRole('button', { name: /Running Verify project/ })).toHaveAttribute('aria-pressed', 'true');
  await sidebar.getByRole('button', { name: 'Drive with Codex' }).click();
  await expect(sidebar.getByRole('button', { name: 'Codex workflow is running' })).toBeVisible();
  const handle = page.getByRole('separator', { name: 'Resize project sidebar' });
  await expect(handle).toHaveAttribute('aria-valuenow', '640');
  const assertDrivePinned = async () => {
    const geometry = await sidebar.evaluate(node => ({ sidebarBottom: node.getBoundingClientRect().bottom, driveBottom: node.querySelector('.drive-control')!.getBoundingClientRect().bottom }));
    expect(geometry.sidebarBottom - geometry.driveBottom).toBeLessThan(16);
  };
  await assertDrivePinned();
  const handleBox = await handle.boundingBox();
  expect(handleBox).not.toBeNull();
  await page.mouse.move(handleBox!.x + handleBox!.width / 2, handleBox!.y + handleBox!.height / 2);
  await page.mouse.down();
  await page.mouse.move(handleBox!.x + handleBox!.width / 2, handleBox!.y - 230, { steps: 5 });
  await page.mouse.up();
  await expect(handle).toHaveAttribute('aria-valuenow', '400');
  const scrollState = await sidebar.evaluate(node => {
    const content = node.querySelector('.project-sidebar__content')!;
    const drive = node.querySelector('.drive-control')!;
    return { contentClientHeight: content.clientHeight, contentScrollHeight: content.scrollHeight, sidebarBottom: node.getBoundingClientRect().bottom, driveBottom: drive.getBoundingClientRect().bottom };
  });
  expect(scrollState.contentScrollHeight).toBeGreaterThan(scrollState.contentClientHeight);
  expect(scrollState.sidebarBottom - scrollState.driveBottom).toBeLessThan(16);
  await handle.focus();
  await page.keyboard.press('ArrowDown');
  await expect(handle).toHaveAttribute('aria-valuenow', '424');
  await assertDrivePinned();
  for (let index = 0; index < 20; index += 1) await page.keyboard.press('ArrowDown');
  await expect(handle).toHaveAttribute('aria-valuenow', '768');
  await assertDrivePinned();
});

test('exercises the application-shell component slice and responsive composition', async ({ page }) => {
  await page.goto('/ux-demo?component=project-tab');
  const tabStates = page.locator('[data-component="project-tab"]');
  await expect(tabStates).toHaveCount(6);
  const selectedLocal = tabStates.filter({ hasText: 'Selected local' });
  await expect(selectedLocal).toHaveAttribute('data-selected', 'true');
  await expect(selectedLocal.locator('[data-lucide="folder-git-2"]')).toHaveCount(0);
  await expect(tabStates.filter({ hasText: 'Remote project' }).locator('[data-lucide="cloud"]')).toHaveCount(1);
  await expect(tabStates.filter({ hasText: 'Busy project' }).locator('.project-tab__busy .loading-spinner')).toHaveCount(1);
  await expect(tabStates.filter({ hasText: 'Needs attention' }).locator('[data-lucide="circle-alert"]')).toHaveCount(1);
  await expect(tabStates.filter({ hasText: 'Disconnected' }).locator('[data-lucide="wifi-off"]')).toHaveCount(1);
  await expect(tabStates.filter({ hasText: 'Not closable' }).getByRole('button', { name: /Close/ })).toHaveCount(0);
  for (const [label, selector] of [['Busy project', '.project-tab__busy'], ['Needs attention', '.project-tab__state--attention'], ['Disconnected', '.project-tab__state']] as const) {
    const tab = tabStates.filter({ hasText: label });
    const geometry = await tab.evaluate((node, stateSelector) => {
      const tab = node.getBoundingClientRect();
      const name = node.querySelector<HTMLElement>('.project-tab__name')!.getBoundingClientRect();
      const state = node.querySelector<HTMLElement>(stateSelector)!.getBoundingClientRect();
      return { rightInset: tab.right - state.right, labelGap: state.left - name.right };
    }, selector);
    expect(geometry.rightInset).toBeGreaterThan(7);
    expect(geometry.labelGap).toBeGreaterThan(0);
  }

  await page.goto('/ux-demo?component=app-tab');
  const sharedTabs=page.locator('[data-component$="-tab"]');await expect(sharedTabs).toHaveCount(2);await expect(page.getByRole('tab',{name:/Project tab/})).toHaveAttribute('aria-selected','true');await expect(page.getByRole('button',{name:'Close Terminal tab'})).toBeAttached();
  await page.goto('/ux-demo?component=terminal-drawer');
  const terminalDrawer=page.locator('[data-component="terminal-drawer"]');await expect(terminalDrawer).toBeVisible();await expect(terminalDrawer.locator('[data-component="terminal-tab"]')).toHaveCount(1);await expect(terminalDrawer.getByRole('button',{name:'Close Development'})).toBeAttached();const gridTab=terminalDrawer.getByRole('tab',{name:'Terminal grid'}),gridWidth=(await gridTab.boundingBox())!.width;await expect(gridTab).toHaveCSS('flex-shrink','0');await terminalDrawer.evaluate(node=>{node.style.width='520px';const tabs=node.querySelector('.terminal-drawer__tabs')!,source=tabs.firstElementChild!;for(let index=0;index<8;index+=1)tabs.append(source.cloneNode(true))});await expect(terminalDrawer.locator('[data-component="terminal-tab"]')).toHaveCount(9);expect((await gridTab.boundingBox())!.width).toBeCloseTo(gridWidth,0);expect(await terminalDrawer.locator('.terminal-drawer__tabs').evaluate(node=>node.scrollWidth)).toBeGreaterThan(await terminalDrawer.locator('.terminal-drawer__tabs').evaluate(node=>node.clientWidth));await terminalDrawer.locator('.terminal-drawer__rail').screenshot({path:'/private/tmp/hs2-029pmj-fixed-grid-tab.png'});

  await page.goto('/ux-demo?component=project-tabs');
  await page.setViewportSize({ width: 1600, height: 900 });
  const tabBar = page.locator('[data-component="project-tab-bar"]');
  await expect(tabBar.getByRole('tab')).toHaveCount(4);
  await expect(tabBar.locator('[data-component="project-tab"]').first()).toHaveCSS('border-radius', '999px');
  const firstClose = tabBar.getByRole('button', { name: 'Close Hot Sheet 2' });
  await expect(firstClose).toHaveCSS('opacity', '0');
  await tabBar.getByRole('tab', { name: /Hot Sheet 2/ }).hover();
  await expect(firstClose).toHaveCSS('opacity', '1');
  const closeGeometry = await tabBar.locator('[data-component="project-tab"]').first().evaluate(node => {
    const close = node.querySelector<HTMLElement>('.project-tab__close')!.getBoundingClientRect();
    const select = node.querySelector<HTMLElement>('.project-tab__select')!.getBoundingClientRect();
    const name = node.querySelector<HTMLElement>('.project-tab__name')!.getBoundingClientRect();
    return { closeWidth: close.width, closeLeft: close.left, selectLeft: select.left, nameLeft: name.left, closeBackground: getComputedStyle(node.querySelector('.project-tab__close')!).backgroundColor, selectPaddingRight: getComputedStyle(node.querySelector('.project-tab__select')!).paddingRight };
  });
  expect(closeGeometry.closeWidth).toBeCloseTo(20.4, 0);
  expect(closeGeometry.closeLeft).toBeLessThan(closeGeometry.selectLeft);
  expect(closeGeometry.nameLeft).toBeGreaterThan(closeGeometry.selectLeft);
  expect(closeGeometry.closeBackground).toBe('rgba(0, 0, 0, 0)');
  expect(closeGeometry.selectPaddingRight).toBe('12px');
  const firstTab=tabBar.locator('[data-component="project-tab"]').first(),firstSelect=firstTab.getByRole('tab');
  await firstSelect.focus();await expect(firstSelect).toBeFocused();
  const focusPresentation=await firstTab.evaluate(node=>{const root=getComputedStyle(node),select=getComputedStyle(node.querySelector('.app-tab__select')!);return{rootOutlineStyle:root.outlineStyle,rootOutlineWidth:root.outlineWidth,rootOutlineOffset:root.outlineOffset,selectOutlineStyle:select.outlineStyle}});
  expect(focusPresentation).toMatchObject({rootOutlineStyle:'solid',rootOutlineWidth:'3px',rootOutlineOffset:'-2px',selectOutlineStyle:'none'});
  await page.screenshot({path:'/private/tmp/hs2-mrz10b-project-tab-focus-wide.png',fullPage:true});
  const tabActionCenters = await tabBar.evaluate(node => {
    const tab = node.querySelector('[data-component="project-tab"]')!.getBoundingClientRect();
    const add = node.querySelector('[data-action="add-project"] svg')!.getBoundingClientRect();
    return { tab: tab.y + tab.height / 2, add: add.y + add.height / 2 };
  });
  expect(tabActionCenters.add).toBeCloseTo(tabActionCenters.tab, 0);
  const order = await tabBar.locator(':scope > *').evaluateAll(nodes => nodes.map(node => node.className));
  expect(order).toEqual(['project-tab-bar__modes', 'project-tab-bar__tabs', 'project-tab-bar__actions']);
  await expect(tabBar.getByRole('button', { name: 'More projects' })).toHaveCount(0);
  const busySpinner = tabBar.getByRole('tab', { name: /Small Tale Website/ }).locator('.project-tab__busy');
  const spinnerAlignment = await busySpinner.evaluate(node => {
    const outer = node.getBoundingClientRect(); const icon = node.querySelector('svg')!.getBoundingClientRect(); const tab = node.closest('[data-component="project-tab"]')!.getBoundingClientRect();
    return { x: Math.abs((outer.left + outer.width / 2) - (icon.left + icon.width / 2)), y: Math.abs((outer.top + outer.height / 2) - (icon.top + icon.height / 2)), tabY: Math.abs((outer.top + outer.height / 2) - (tab.top + tab.height / 2)) };
  });
  expect(spinnerAlignment.x).toBeLessThan(1);
  expect(spinnerAlignment.y).toBeLessThan(1);
  expect(spinnerAlignment.tabY).toBeLessThan(1);
  const animatedCenters = await busySpinner.evaluate(async node => {
    const centers: Array<[number, number]> = [];
    for (let frame = 0; frame < 12; frame++) await new Promise<void>(resolve => requestAnimationFrame(() => { const box = node.querySelector('svg')!.getBoundingClientRect(); centers.push([box.x + box.width / 2, box.y + box.height / 2]); resolve(); }));
    return centers;
  });
  expect(Math.max(...animatedCenters.map(([x]) => x)) - Math.min(...animatedCenters.map(([x]) => x))).toBeLessThan(.1);
  expect(Math.max(...animatedCenters.map(([, y]) => y)) - Math.min(...animatedCenters.map(([, y]) => y))).toBeLessThan(.1);
  for (const name of await tabBar.locator('.project-tab__name').all()) {
    await expect(name).not.toHaveCSS('text-overflow', 'ellipsis');
  }
  await page.setViewportSize({ width: 760, height: 900 });
  const overflowState = await tabBar.evaluate(node => {
    const strip = node.querySelector<HTMLElement>('.project-tab-bar__tabs')!;
    const selected = node.querySelector<HTMLElement>('.project-tab[data-selected="true"]')!.getBoundingClientRect();
    const viewport = strip.getBoundingClientRect();
    strip.scrollLeft = 100;
    return { clientWidth: strip.clientWidth, scrollWidth: strip.scrollWidth, scrollLeft: strip.scrollLeft, overflowX: getComputedStyle(strip).overflowX, shadowClearance: viewport.bottom - selected.bottom };
  });
  expect(overflowState.scrollWidth).toBeGreaterThan(overflowState.clientWidth);
  expect(overflowState.scrollLeft).toBeGreaterThan(0);
  expect(overflowState.overflowX).toBe('auto');
  expect(overflowState.shadowClearance).toBeGreaterThanOrEqual(3);
  await tabBar.locator('.project-tab-bar__tabs').evaluate(node=>{node.scrollLeft=0});await firstSelect.focus();await expect(firstSelect).toBeFocused();await page.screenshot({path:'/private/tmp/hs2-mrz10b-project-tab-focus-narrow.png',fullPage:true});
  await page.setViewportSize({ width: 1280, height: 900 });
  await tabBar.getByRole('tab', { name: /Hot Sheet 2/ }).click({ button: 'right' });
  const tabMenu = page.getByRole('menu', { name: 'Project tab actions' });
  await expect(tabMenu.locator('wa-dropdown-item')).toHaveCount(4);
  await expect(tabMenu.locator('[data-lucide]')).toHaveCount(4);
  await page.keyboard.press('Escape');
  await expect(tabMenu).toHaveCount(0);
  await tabBar.getByRole('tab', { name: /Hot Sheet 2/ }).click({ button: 'right' });
  await tabMenu.locator('wa-dropdown-item', { hasText: 'Close Tabs to the Right' }).click();
  await expect(tabBar.getByRole('tab')).toHaveCount(1);
  await page.setViewportSize({ width: 1600, height: 900 });
  await page.reload();
  await expect(tabBar.getByRole('tab')).toHaveCount(4);
  await tabBar.getByRole('tab', { name: /Internal API/ }).click();
  await expect(tabBar.getByRole('tab', { name: /Internal API/ })).toHaveAttribute('aria-selected', 'true');
  await page.keyboard.press('ArrowLeft');
  await expect(tabBar.getByRole('tab', { name: /Small Tale Website/ })).toHaveAttribute('aria-selected', 'true');
  await expect(tabBar.getByRole('tab', { name: /Small Tale Website/ })).toBeFocused();
  await tabBar.getByRole('tab', { name: /Internal API/ }).click();
  await tabBar.getByRole('button', { name: 'Close Internal API' }).click();
  await expect(tabBar.getByRole('tab', { name: /Internal API/ })).toHaveCount(0);
  await tabBar.getByRole('button', { name: 'Add project' }).click();
  await expect(tabBar.getByRole('tab', { name: /New Project 1/ })).toHaveAttribute('aria-selected', 'true');
  await tabBar.getByRole('button', { name: 'Terminal dashboard' }).click();
  await expect(tabBar.getByRole('button', { name: 'Terminal dashboard' })).toHaveAttribute('aria-pressed', 'true');
  await expect(tabBar.getByRole('tab', { selected: true })).toHaveCount(0);
  await tabBar.getByRole('tab', { name: /Hot Sheet 2/ }).click();
  await expect(tabBar.getByRole('button', { name: 'Terminal dashboard' })).toHaveAttribute('aria-pressed', 'false');

  await page.goto('/ux-demo?component=resizable-region');
  const horizontal = page.getByRole('separator', { name: 'Resize Example sidebar' });
  const vertical = page.getByRole('separator', { name: 'Resize Example drawer' });
  await expect(horizontal).toHaveAttribute('aria-orientation', 'vertical');
  await expect(vertical).toHaveAttribute('aria-orientation', 'horizontal');
  await horizontal.focus();
  await page.keyboard.press('ArrowRight');
  await expect(horizontal).toHaveAttribute('aria-valuenow', '276');
  for (let index = 0; index < 20; index += 1) await page.keyboard.press('ArrowLeft');
  await expect(horizontal).toHaveAttribute('aria-valuenow', '250');
  await vertical.focus();
  await page.keyboard.press('ArrowDown');
  await expect(vertical).toHaveAttribute('aria-valuenow', '196');
  await expect(horizontal).toHaveCSS('cursor', 'col-resize');
  await expect(vertical).toHaveCSS('cursor', 'row-resize');
  await page.getByRole('button', { name: 'Collapse horizontal region' }).click();
  await expect(horizontal).toHaveAttribute('aria-valuenow', '0');
  await expect(horizontal).toHaveAttribute('aria-valuemin', '0');
  await page.getByRole('button', { name: 'Restore horizontal region' }).click();
  await expect(horizontal).toHaveAttribute('aria-valuenow', '250');

  await page.goto('/ux-demo?component=connection-state-banner');
  const banners = page.locator('[data-component="connection-state-banner"]');
  await expect(banners).toHaveCount(5);
  await expect(page.locator('[data-component="connection-state-banner"][data-state="connecting"]')).toHaveAttribute('role', 'status');
  await expect(page.locator('[data-component="connection-state-banner"][data-state="offline"]')).toContainText('Working from offline data');
  await expect(page.locator('[data-component="connection-state-banner"][data-state="incompatible"]')).toContainText('Server update required');
  await expect(page.locator('[data-component="connection-state-banner"][data-state="authentication"]')).toContainText('Authentication required');
  await page.getByRole('button', { name: 'Reconnect' }).click();
  await expect(page.getByText('Connection retry requested.')).toBeVisible();

  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.goto('/ux-demo?component=app-shell');
  const shell = page.locator('[data-component="app-shell"]');
  await expect(shell).toBeVisible();
  for (const component of ['project-sidebar', 'project-tab-bar', 'connection-state-banner', 'workspace-identity', 'workspace-controls', 'quick-ticket-composer-launcher', 'ticket-list', 'ticket-inspector']) await expect(shell.locator(`[data-component="${component}"]`)).toHaveCount(1);
  const shellHierarchy = await shell.evaluate(node => {
    const shellRect = node.getBoundingClientRect();
    const toolbarNode = node.querySelector('.app-shell__main > [data-component="toolbar"]')!;
    const toolbar = toolbarNode.getBoundingClientRect();
    const leading = toolbarNode.querySelector('.toolbar__leading')!.getBoundingClientRect();
    const trailing = toolbarNode.querySelector('.toolbar__trailing')!.getBoundingClientRect();
    const identity = toolbarNode.querySelector('[data-component="workspace-identity"]')!.getBoundingClientRect();
    const controls = toolbarNode.querySelector('[data-component="workspace-controls"]')!.getBoundingClientRect();
    const tabs = node.querySelector('.project-tab-bar')!.getBoundingClientRect();
    const pageHeader = node.querySelector('.page-header')!.getBoundingClientRect();
    const inspector = node.querySelector('.ticket-inspector')!.getBoundingClientRect();
    return { shellTop: shellRect.top, toolbarTop: toolbar.top, toolbarBottom: toolbar.bottom, toolbarRight: toolbar.right, toolbarGap: getComputedStyle(toolbarNode).columnGap, leadingLeft: leading.left, identityLeft: identity.left, trailingRight: trailing.right, controlsRight: controls.right, tabsTop: tabs.top, tabsBottom: tabs.bottom, pageHeaderTop: pageHeader.top, inspectorTop: inspector.top };
  });
  expect(shellHierarchy.toolbarTop - shellHierarchy.shellTop).toBeLessThanOrEqual(1);
  expect(shellHierarchy.identityLeft).toBeGreaterThanOrEqual(shellHierarchy.leadingLeft);
  expect(shellHierarchy.controlsRight).toBeCloseTo(shellHierarchy.trailingRight, 0);
  expect(shellHierarchy.toolbarRight - shellHierarchy.trailingRight).toBeCloseTo(16, 0);
  expect(shellHierarchy.toolbarGap).toBe('0px');
  await expect(shell.locator('.app-shell__main > [data-component="toolbar"]')).toHaveAttribute('data-has-center', 'false');
  await expect(shell.locator('.app-shell__main > [data-component="toolbar"]')).toHaveCSS('border-bottom-color', 'rgba(0, 0, 0, 0)');
  expect(shellHierarchy.tabsTop).toBeCloseTo(shellHierarchy.toolbarBottom, 0);
  expect(shellHierarchy.pageHeaderTop).toBeGreaterThanOrEqual(shellHierarchy.tabsBottom);
  expect(shellHierarchy.inspectorTop - shellHierarchy.shellTop).toBeLessThanOrEqual(1);
  await page.screenshot({ path: '/private/tmp/hs2-501eph-toolbar-wide.png', fullPage: true });
  await expect(shell.getByRole('button', { name: 'Hide inspector' }).locator('[data-lucide="panel-right-close"]')).toHaveCount(1);
  await expect(shell.locator('.project-tab-bar')).toHaveCSS('background-color', 'rgb(255, 255, 255)');
  await expect(shell.locator('.project-tab-bar')).toHaveCSS('border-bottom-width', '0px');
  await expect(shell.locator('[data-component="project-sidebar"]')).toHaveCSS('background-color', 'rgb(255, 255, 255)');
  await shell.getByRole('button', { name: /New ticket/ }).click();
  const shellComposer=page.getByRole('dialog',{name:'Create ticket'});
  await expect(shellComposer.getByRole('textbox', { name: 'Ticket title' })).toBeFocused();
  const composerControlHeights = await shellComposer.evaluate(node => {
    const input = node.querySelector('wa-input')!.shadowRoot!.querySelector<HTMLElement>('[part~="base"]')!.getBoundingClientRect();
    const select = node.querySelector('wa-select')!.shadowRoot!.querySelector<HTMLElement>('[part~="combobox"]')!.getBoundingClientRect();
    return { input: input.height, select: select.height };
  });
  expect(composerControlHeights.input).toBeCloseTo(composerControlHeights.select, 0);
  await shellComposer.getByRole('button', { name: 'Cancel' }).click();
  await shell.getByRole('button', { name: /New ticket/ }).click();
  await expect(shellComposer.getByRole('textbox', { name: 'Ticket title' })).toBeFocused();
  await shellComposer.getByRole('button', { name: 'Cancel' }).click();
  const inspectorToolbarAlignment = await shell.locator('.ticket-inspector__header > [data-component="toolbar"]').evaluate(node => {
    const slug = node.querySelector('[data-component="toolbar-text"]')!.getBoundingClientRect();
    const controls = node.querySelector('[data-component="toolbar-control-group"]')!.getBoundingClientRect();
    return Math.abs((slug.top + slug.height / 2) - (controls.top + controls.height / 2));
  });
  expect(inspectorToolbarAlignment).toBeLessThan(1);
  const compactInspectorTab = shell.getByRole('button', { name: 'Timeline' });
  await expect(compactInspectorTab.locator('svg')).toHaveCSS('flex-shrink', '0');
  await expect(compactInspectorTab.locator('span')).toBeHidden();
  const sidebarHandle = shell.getByRole('separator', { name: 'Resize Project sidebar' });
  await expect(sidebarHandle).toHaveAttribute('aria-valuemin', '250');
  await expect(sidebarHandle).toHaveAttribute('aria-valuenow', '272');
  await sidebarHandle.focus();
  await page.keyboard.press('ArrowRight');
  await expect(sidebarHandle).toHaveAttribute('aria-valuenow', '288');
  const inspectorHandle = shell.getByRole('separator', { name: 'Resize Ticket inspector' });
  await inspectorHandle.focus();
  await page.keyboard.press('ArrowLeft');
  await expect(inspectorHandle).toHaveAttribute('aria-valuenow', '368');
  await page.keyboard.press('ArrowRight');
  await expect(inspectorHandle).toHaveAttribute('aria-valuenow', '352');
  const inspectorHandleBox = await inspectorHandle.boundingBox();
  expect(inspectorHandleBox).not.toBeNull();
  await shell.locator('[data-component="ticket-list-row"]').first().evaluate(node => { (node as HTMLElement).dataset.resizeStability = 'same-node'; });
  await page.mouse.move(inspectorHandleBox!.x + inspectorHandleBox!.width / 2, inspectorHandleBox!.y + 80);
  const inspectorRegion = shell.locator(':scope > [data-component="resizable-region"][data-region-id="app-inspector"]');
  // HS2-4KZBTT: the region width must never animate — animating it reflows the whole ticket list
  // per frame. The content still slides via a compositor-only transform.
  await expect(inspectorRegion).toHaveCSS('transition-duration', '0s');
  await page.mouse.down();
  // While dragging, the resize guard suppresses even the content transform transition.
  await expect(inspectorRegion.locator('.resizable-region__content')).toHaveCSS('transition-duration', '0s');
  await page.mouse.move(inspectorHandleBox!.x - 32, inspectorHandleBox!.y + 80);
  await expect(shell.locator('[data-resize-stability="same-node"]')).toHaveCount(1);
  await page.mouse.up();
  // After the drag ends the content transition returns, but the region width stays un-animated.
  await expect(inspectorRegion.locator('.resizable-region__content')).not.toHaveCSS('transition-duration', '0s');
  await expect(inspectorRegion).toHaveCSS('transition-duration', '0s');
  await expect.poll(async () => Number(await inspectorHandle.getAttribute('aria-valuenow'))).toBeGreaterThan(352);
  await shell.getByRole('button', { name: 'Timeline' }).click();
  await expect(shell.getByRole('button', { name: 'Timeline' })).toHaveAttribute('aria-current', 'page');
  await expect(shell.locator('[data-component="ticket-timeline"]')).toBeVisible();
  await shell.getByRole('button', { name: 'Attachments' }).click();
  await expect(shell.locator('[data-component="ticket-attachments"]')).toBeVisible();
  await shell.getByRole('button', { name: 'Info' }).click();
  await expect(shell.locator('[data-component="ticket-info-panel"]')).toBeVisible();
  const inspectorExpandedWidth = Number(await inspectorHandle.getAttribute('aria-valuenow'));
  await shell.getByRole('button', { name: 'Hide inspector' }).click();
  const showInspector = shell.getByRole('button', { name: 'Show ticket inspector' });
  await expect(showInspector).toBeVisible();
  const collapsedInspector = shell.locator(':scope > [data-component="resizable-region"][data-region-id="app-inspector"]');
  await expect(collapsedInspector).toHaveAttribute('data-collapsed', 'true');
  await expect(collapsedInspector).toHaveCSS('width', '0px');
  await expect(collapsedInspector.locator('.resizable-region__content')).toHaveCSS('width', `${inspectorExpandedWidth}px`);
  await expect(collapsedInspector.locator('.resizable-region__content')).not.toHaveCSS('transform', 'none');
  await expect(showInspector.locator('[data-lucide="panel-right-open"]')).toHaveCount(1);
  await expect(showInspector.locator('xpath=ancestor::*[@data-component="project-tab-bar"]')).toHaveCount(0);
  await expect(showInspector.locator('xpath=ancestor::*[@data-component="toolbar"]')).toHaveCount(1);
  await showInspector.click();
  await expect(shell.locator('[data-component="ticket-inspector"]')).toBeVisible();
  const hideSidebar = shell.getByRole('button', { name: 'Hide project sidebar' });
  const crampedToolbar = await shell.locator('.app-shell__main > [data-component="toolbar"]').evaluate(toolbar => {
    const toolbarRect = toolbar.getBoundingClientRect();
    const actionsRect = toolbar.querySelector('[data-component="workspace-controls"]')!.getBoundingClientRect();
    return {
      toolbarLeft: toolbarRect.left,
      toolbarRight: toolbarRect.right,
      actionsLeft: actionsRect.left,
      actionsRight: actionsRect.right,
    };
  });
  expect(crampedToolbar.actionsLeft).toBeGreaterThanOrEqual(crampedToolbar.toolbarLeft);
  expect(crampedToolbar.actionsRight).toBeLessThanOrEqual(crampedToolbar.toolbarRight);
  const sidebarCollapseHit = await hideSidebar.evaluate(button => {
    const rect = button.getBoundingClientRect();
    const hit = document.elementFromPoint(rect.left + rect.width / 2, rect.top + rect.height / 2);
    return {
      ownsHit: hit?.closest('[aria-label="Hide project sidebar"]') === button,
      hitLabel: hit?.closest('[aria-label]')?.getAttribute('aria-label'),
    };
  });
  expect(sidebarCollapseHit).toEqual({ ownsHit: true, hitLabel: 'Hide project sidebar' });
  await page.screenshot({ path: '/private/tmp/hs2-501eph-toolbar-narrow.png', fullPage: true });
  await hideSidebar.click();
  await expect(shell.locator('[data-component="resizable-region"][data-region-id="app-sidebar"]')).toHaveAttribute('data-collapsed', 'true');
  await expect(shell.locator('[data-component="resizable-region"][data-region-id="app-sidebar"]')).toHaveCSS('width', '0px');
  const collapsedSidebarContent = shell.locator('[data-component="resizable-region"][data-region-id="app-sidebar"] .resizable-region__content');
  await expect(collapsedSidebarContent).toHaveCSS('width', '288px');
  await expect(collapsedSidebarContent).not.toHaveCSS('transform', 'none');
  const showSidebar = shell.getByRole('button', { name: 'Show project sidebar' });
  await expect(showSidebar.locator('xpath=ancestor::*[@data-component="project-tab-bar"]')).toHaveCount(0);
  await expect(showSidebar.locator('xpath=ancestor::*[@data-component="toolbar"]')).toHaveCount(1);
  await showSidebar.click();
  await expect(shell.locator('[data-component="project-sidebar"]')).toBeVisible();
  await expect(shell.locator('[data-component="resizable-region"][data-region-id="app-sidebar"]')).toHaveAttribute('data-collapsed', 'false');
  await expect(shell.locator(':scope > [data-component="resizable-region"][data-region-id="app-sidebar"]')).toHaveCSS('border-right-width', '1px');
  const sidebarSeparator = await shell.locator(':scope > [data-component="resizable-region"][data-region-id="app-sidebar"]').evaluate(node => ({ width: getComputedStyle(node, '::after').width, background: getComputedStyle(node, '::after').backgroundColor }));
  expect(sidebarSeparator).toEqual({ width: '1px', background: 'rgb(209, 209, 214)' });
  await shell.getByRole('button', { name: 'Columns view' }).click();
  const shellWorkspace = shell.locator('.app-shell__workspace');
  await expect(shellWorkspace).toHaveAttribute('data-presentation', 'edge-to-edge');
  const boardGeometry = await shellWorkspace.evaluate(node => {
    const board = node.querySelector('.ticket-board')!;
    const workspaceRect = node.getBoundingClientRect();
    const boardRect = board.getBoundingClientRect();
    return { workspaceLeft: workspaceRect.left, workspaceRight: workspaceRect.right, workspaceBottom: workspaceRect.bottom, boardLeft: boardRect.left, boardRight: boardRect.right, boardBottom: boardRect.bottom, boardClientWidth: board.clientWidth, boardScrollWidth: board.scrollWidth };
  });
  expect(boardGeometry.boardLeft - boardGeometry.workspaceLeft).toBeCloseTo(0, 0);
  expect(boardGeometry.workspaceRight - boardGeometry.boardRight).toBeCloseTo(0, 0);
  expect(boardGeometry.workspaceBottom - boardGeometry.boardBottom).toBeCloseTo(0, 0);
  expect(boardGeometry.boardScrollWidth).toBeGreaterThanOrEqual(boardGeometry.boardClientWidth);
  await shell.getByRole('button', { name: 'List view' }).click();
  await shell.getByRole('tab', { name: /Small Tale Website/ }).click();
  await expect(shell.locator('.project-tab[data-project-id="website"] [role="tab"]')).toHaveAttribute('aria-selected', 'true');
  await shell.getByRole('button', { name: 'Settings view' }).click();
  await expect(shell.getByRole('region', { name: 'Project settings' })).toBeVisible();
  await expect(shell.locator('[data-component="ticket-list"]')).toHaveCount(0);
  await expect(shell.locator('[data-component="quick-ticket-composer"]')).toHaveCount(0);
  await expect(shell.getByRole('complementary', { name: 'Ticket inspector' })).toBeVisible();
  for (const name of ['Sort tickets', 'Favorite view', 'More workspace actions', 'Search tickets']) {
    const control=shell.getByRole('button', { name });
    if (await control.count()) await expect(control).toHaveAttribute('disabled', '');
  }
  await shell.getByRole('button', { name: 'List view' }).click();
  await expect(shell.locator('[data-component="ticket-list"]')).toBeVisible();
  await expect(shell.locator('[data-component="quick-ticket-composer-launcher"]')).toBeVisible();
  await expect(shell.locator('[data-component="ticket-inspector"]')).toBeVisible();
  await shell.getByRole('button', { name: 'Search tickets' }).click();
  const shellSearch = shell.getByRole('textbox', { name: 'Search tickets' });
  await expect(shellSearch).toBeFocused();
  await shellSearch.fill('long-tag-example');
  await expect(shell.locator('[data-component="ticket-list-row"]')).toHaveCount(1);
  await shellSearch.fill('');
  await shellSearch.blur();
  await expect(shell.getByRole('textbox', { name: 'Search tickets' })).toHaveCount(0);
  await expect(shell.getByRole('button', { name: 'Search tickets' })).toBeVisible();
  await shell.getByRole('button', { name: 'Terminal dashboard' }).click();
  await expect(shell).toHaveAttribute('data-mode', 'terminals');
  await expect(shell.locator('[data-component="project-sidebar"]')).toHaveCount(0);
  await expect(shell.getByRole('complementary', { name: 'Terminal operations sidebar' })).toBeVisible();
  await expect(shell.getByRole('button', { name: 'Hide operations sidebar' })).toBeVisible();
  await expect(shell.getByRole('region', { name: 'Ticket rail' })).toBeVisible();
  await expect(shell.locator('[data-component="ticket-inspector"]')).toBeVisible();
  await expect(shell.locator('[data-component="quick-ticket-composer"]')).toHaveCount(0);
  await expect(shell.getByText('Terminals', { exact: true })).toBeVisible();
  await expect(shell.getByRole('region', { name: 'Terminal dashboard workspace' })).toBeVisible();
  await expect(shell.locator('.workspace-header__actions')).toHaveCount(0);
  await shell.getByRole('button', { name: 'Cross-project stats' }).click();
  await expect(shell).toHaveAttribute('data-mode', 'stats');
  await expect(shell.getByText('Stats', { exact: true })).toBeVisible();
  await expect(shell.getByRole('region', { name: 'Cross-project stats workspace' })).toBeVisible();
  await shell.getByRole('tab', { name: /Hot Sheet 2/ }).click();
  await expect(shell).toHaveAttribute('data-mode', 'project');
  await expect(shell.locator('[data-component="project-sidebar"]')).toBeVisible();
  await expect(shell.locator('[data-component="ticket-inspector"]')).toBeVisible();
  await expect(shell.locator('.workspace-header__actions')).toBeVisible();
  await shell.locator('[data-component="project-summary"]').click();
  await expect(shell).toHaveAttribute('data-mode','stats');
  await expect(shell.getByRole('region',{name:'Hot Sheet 2 project statistics'})).toBeVisible();
  await shell.getByRole('tab',{name:/Hot Sheet 2/}).click();
  await expect(shell).toHaveAttribute('data-mode','project');
  await page.setViewportSize({ width: 760, height: 900 });
  await expect(shell.locator(':scope > [data-component="resizable-region"][data-region-id="app-sidebar"]')).toBeVisible();
  await expect(shell.locator(':scope > [data-component="resizable-region"][data-region-id="app-inspector"]')).toBeVisible();
  // Keep all three user-controlled regions mounted at the supported floor. The
  // center can be clipped until the user explicitly collapses a side region.
  await expect(shell.locator('[data-component="ticket-list"]')).toHaveCount(1);
});

test('projects the feedback-needed indicator through list and board compositions', async ({ page }) => {
  await page.goto('/ux-demo?component=ticket-list');
  const listFeedback = page.getByRole('listbox', { name: 'Example ticket list' })
    .locator('[data-ticket-slug="HS2-R76MMW"] .ticket-list-row__feedback');
  await expect(listFeedback).toContainText('Needs review');
  await expect(listFeedback.locator('[data-lucide="circle-alert"]')).toHaveCount(1);
  const listRow = page.getByRole('listbox', { name: 'Example ticket list' }).locator('[data-ticket-slug="HS2-R76MMW"]');
  await expect(listRow.locator('.ticket-list-row__indicator--needs-review')).toHaveCSS('background-color', 'rgb(203, 48, 224)');
  // A ticket without a feedback_needed note shows no indicator.
  await expect(page.getByRole('listbox', { name: 'Example ticket list' })
    .locator('[data-ticket-slug="HS2-RPVFA4"] .ticket-list-row__feedback')).toHaveCount(0);

  await page.goto('/ux-demo?component=ticket-board');
  const columnRow = page.getByRole('listbox', { name: 'Example status board' }).locator('[data-ticket-slug="HS2-R76MMW"]');
  await expect(columnRow.locator('.ticket-list-row__feedback')).toContainText('Needs review');
  await expect(columnRow.locator('.ticket-list-row__indicator--needs-review')).toHaveCSS('background-color', 'rgb(203, 48, 224)');

  await page.goto('/ux-demo?component=ticket-inspector');
  const inspector = page.locator('[data-component="ticket-inspector"]');
  await expect(inspector).toHaveAttribute('data-needs-review', 'true');
  await expect(inspector.locator('.ticket-inspector__feedback')).toContainText('Needs review');
  expect(await inspector.evaluate(node => {
    const rail = getComputedStyle(node, '::before');
    return { background: rail.backgroundColor, width: rail.width };
  })).toEqual({ background: 'rgba(0, 0, 0, 0)', width: 'auto' });
});

test('dims finished tickets in the list and gives the add-tag control full width', async ({ page }) => {
  await page.goto('/ux-demo?component=ticket-list');
  const list = page.getByRole('listbox', { name: 'Example ticket list' });
  // Completed/verified rows are dimmed in list mode; active rows are not (HS2-AMBE59).
  await expect(list.locator('[data-ticket-slug="HS2-K00QPZ"]')).toHaveCSS('opacity', '0.55');
  await expect(list.locator('[data-ticket-slug="HS2-RPVFA4"]')).toHaveCSS('opacity', '0.55');
  await expect(list.locator('[data-ticket-slug="HS2-R76MMW"]')).toHaveCSS('opacity', '1');

  // The add-tag control spans the full width of the tag editor (HS2-MBX8MV).
  await page.goto('/ux-demo?component=ticket-info-panel');
  const [add, editor] = await Promise.all([
    page.locator('.ticket-tag-editor__add').boundingBox(),
    page.locator('.ticket-tag-editor').boundingBox(),
  ]);
  expect(add!.width).toBeCloseTo(editor!.width, 0);
});

test('resolves the shared Web Awesome and Hot Sheet semantic theme', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto('/ux-demo?component=ticket-list');
  const row = page.locator('[data-ticket-slug="HS2-R76MMW"]');
  const feedback = row.locator('.ticket-list-row__feedback');
  await expect(row).toBeVisible();
  await expect(feedback).toBeVisible();

  expect(await row.evaluate(node => {
    const root = getComputedStyle(document.documentElement);
    const probe = document.createElement('span');
    probe.style.backgroundColor = 'var(--wa-color-surface-default)';
    document.body.append(probe);
    const surface = getComputedStyle(probe).backgroundColor;
    probe.style.backgroundColor = 'var(--wa-color-warning-fill-quiet)';
    const warning = getComputedStyle(probe).backgroundColor;
    probe.style.backgroundColor = 'var(--hs-ticket-state-needs-review)';
    const review = getComputedStyle(probe).backgroundColor;
    probe.remove();
    const feedbackNode = node.querySelector('.ticket-list-row__feedback');
    const railNode = node.querySelector('.ticket-list-row__indicator--needs-review');
    return {
      aliases: [
        root.getPropertyValue('--hs-shell-divider').trim(),
        root.getPropertyValue('--hs-ticket-state-needs-review').trim(),
        root.getPropertyValue('--hs-ticket-state-up-next').trim(),
      ],
      reviewMatches: railNode !== null && getComputedStyle(railNode).backgroundColor === review,
      surfaceMatches: getComputedStyle(node).backgroundColor === surface,
      warningMatches: feedbackNode !== null && getComputedStyle(feedbackNode).backgroundColor === warning,
    };
  })).toEqual({
    aliases: ['#d1d1d6', '#cb30e0', '#ffcc00'],
    reviewMatches: true,
    surfaceMatches: true,
    warningMatches: true,
  });

  await page.goto('/ux-demo?component=app-shell');
  const workArea = page.locator('.app-shell__work-area');
  await expect(workArea).toBeVisible();
  expect(await workArea.evaluate(node => {
    const probe = document.createElement('span');
    probe.style.backgroundColor = 'var(--wa-color-surface-lowered)';
    document.body.append(probe);
    const matches = getComputedStyle(node).backgroundColor === getComputedStyle(probe).backgroundColor;
    probe.remove();
    return matches;
  })).toBe(true);
  const sidebarRegion = page.locator('.app-shell > .resizable-region[data-region-id="app-sidebar"]');
  expect(await sidebarRegion.evaluate(node => ({
    divider: getComputedStyle(node, '::after').backgroundColor,
    token: getComputedStyle(document.documentElement).getPropertyValue('--hs-shell-divider').trim(),
  }))).toEqual({ divider: 'rgb(209, 209, 214)', token: '#d1d1d6' });
  await page.screenshot({ path: '/private/tmp/hs2-66m88k-semantic-theme-wide.png', fullPage: true });
  await page.setViewportSize({ width: 940, height: 844 });
  await expect(sidebarRegion).toBeVisible();
  await page.screenshot({ path: '/private/tmp/hs2-66m88k-semantic-theme-narrow.png', fullPage: true });
  await page.setViewportSize({ width: 1280, height: 900 });

  await page.goto('/ux-demo?component=permission-request');
  const details = page.locator('.permission-request-card__details');
  await expect(details).toBeVisible();
  await expect(details).toHaveCSS('background-color', await page.evaluate(() => {
    const probe = document.createElement('span');probe.style.backgroundColor = 'var(--wa-color-surface-lowered)';document.body.append(probe);const color = getComputedStyle(probe).backgroundColor;probe.remove();return color;
  }));
  await page.screenshot({ path: '/private/tmp/hotsheet-semantic-theme-wide.png', fullPage: true });

  await page.setViewportSize({ width: 760, height: 900 });
  await expect(details).toBeVisible();
  await page.screenshot({ path: '/private/tmp/hotsheet-semantic-theme-narrow.png', fullPage: true });
});

test('previews and resets important PermissionRequestCard variants', async ({ page }) => {
  let releaseDemoModified!: () => void;
  const demoModifiedReady = new Promise<void>(resolve => { releaseDemoModified = resolve; });
  await page.route('**/__hotsheet/demo-modified', async route => {
    await demoModifiedReady;
    await route.fulfill({ json: { 'permission-request': new Date().toISOString() } });
  });
  await page.setViewportSize({ width: 1728, height: 971 });
  await page.goto('/ux-demo?component=permission-request');
  await page.getByRole('button', { name: 'Settings', exact: true }).click();
  const settings = page.getByRole('complementary', { name: 'PermissionRequestCard settings' });
  const card = page.locator('[data-component="permission-request-card"]');
  const presentation = settings.locator('[name="presentation"]');
  const variant = settings.locator('[name="variant"]');
  const request = settings.locator('[name="request"]');
  const automation = settings.locator('[name="automation"]');
  const alwaysSupported = settings.locator('wa-checkbox[name="always-supported"]');
  const explanation = settings.locator('wa-checkbox[name="explanation"]');
  const choose = async (control: typeof variant, value: string) => control.evaluate((node: HTMLElement & { value: string }, selected) => {
    node.value = selected;
    node.dispatchEvent(new Event('change', { bubbles: true }));
  }, value);

  await expect(page.locator('[data-component="permission-request-popup"]')).toBeVisible();
  await expect(card).toHaveAttribute('data-state', 'pending');
  await expect(card).toContainText('Wants permission to edit');
  await expect(card).toContainText('Auto-allow in');
  await presentation.evaluate(async node => {
    await customElements.whenDefined('wa-select');
    await (node as HTMLElement & { updateComplete?: Promise<unknown> }).updateComplete;
  });
  await presentation.evaluate(node => { (window as typeof window & { __permissionPresentation?: Element }).__permissionPresentation = node; });
  await presentation.click();
  await expect(presentation).toHaveJSProperty('open', true);
  releaseDemoModified();
  const initialCountdown = await card.locator('.permission-request-card__timer').textContent();
  await expect.poll(() => card.locator('.permission-request-card__timer').textContent()).not.toBe(initialCountdown);
  await expect(presentation).toHaveJSProperty('open', true);
  expect(await presentation.evaluate(node => (window as typeof window & { __permissionPresentation?: Element }).__permissionPresentation === node)).toBe(true);
  await page.keyboard.press('Escape');
  await expect(presentation).toHaveJSProperty('open', false);
  await expect(page.locator('[data-action="select-demo"][data-item-id="permission-request"] small')).toHaveText('Now');
  await choose(presentation, 'list');
  await expect(page.locator('[data-component="permission-request-popup"]')).toHaveCount(0);
  await choose(presentation, 'popup');
  await expect(page.locator('[data-component="permission-request-popup"]')).toBeVisible();
  const stopAutomation = card.getByRole('button', { name: 'Stop auto-allow countdown' });
  await expect(stopAutomation).toHaveAttribute('title', 'Stop auto-allow countdown for this request');
  await expect(stopAutomation.locator('[data-lucide="pause"]')).toBeVisible();
  expect((await stopAutomation.textContent())?.trim()).toBe('');
  await expect(card.locator('.permission-request-card__countdown')).toHaveCSS('border-top-style', 'none');
  const [timerBox, denyBox] = await Promise.all([card.locator('.permission-request-card__timer').boundingBox(), card.getByRole('button', { name: 'Deny' }).boundingBox()]);
  expect(Math.abs((timerBox!.y + timerBox!.height / 2) - (denyBox!.y + denyBox!.height / 2))).toBeLessThanOrEqual(1);
  await page.screenshot({ path: '/private/tmp/hs2-xrva64-permission-countdown-wide.png', fullPage: true });
  await stopAutomation.click();
  await expect(card.locator('.permission-request-card__countdown')).toHaveCount(0);
  await expect(automation).toHaveJSProperty('value', 'none');
  await choose(automation, 'allow');
  await expect(card.getByRole('button', { name: 'Always Allow' })).toBeVisible();
  await expect(card.locator('.permission-request-card__explanation')).toBeVisible();

  await choose(presentation, 'list');
  await expect(page.locator('[data-component="permission-request-popup"]')).toHaveCount(0);
  await expect(card).toHaveClass(/permission-request-card--list/);
  await choose(variant, 'resolving');
  await expect(card).toHaveAttribute('data-state', 'resolving');
  await expect(card.getByRole('button', { name: 'Deny' })).toBeDisabled();
  await choose(variant, 'failed');
  await expect(card).toContainText('could not be delivered');
  await choose(variant, 'disconnected');
  await expect(card).toContainText('disconnected before this request was answered');
  await choose(variant, 'allowed');
  await expect(card).toHaveAttribute('data-state', 'allow');
  await expect(card).toContainText('allowed this kind of request');
  await expect(card.getByRole('button', { name: 'Deny' })).toHaveCount(0);
  await choose(request, 'tool-without-details');
  await expect(card.locator('.permission-request-card__details')).toHaveCount(0);
  await expect(card.locator('.permission-request-card__footer')).toHaveCount(0);
  await expect(card).toHaveCSS('padding-bottom', '16px');
  await choose(variant, 'denied');
  await expect(card).toHaveAttribute('data-state', 'deny');
  await choose(variant, 'external');
  await expect(card).toContainText('Decision made outside Hot Sheet');

  await choose(variant, 'pending');
  await choose(request, 'command');
  await expect(card).toContainText('Wants permission to run a command');
  await expect(card.locator('.permission-request-card__details')).toContainText('npm run test:unit');
  await choose(request, 'read');
  await expect(card).toContainText('Wants permission to read');
  await choose(request, 'tool-without-details');
  await expect(card).toContainText('Wants permission to use ToolSearch');
  await expect(card.locator('.permission-request-card__details')).toHaveCount(0);
  await choose(automation, 'deny');
  await expect(card).toContainText('Auto-deny in');
  await choose(automation, 'none');
  await expect(card.locator('.permission-request-card__countdown')).toHaveCount(0);
  await alwaysSupported.click();
  await expect(card.getByRole('button', { name: 'Always Allow' })).toHaveCount(0);
  await expect(card.getByRole('button', { name: 'Allow', exact: true })).toBeVisible();
  await explanation.click();
  await expect(card.locator('.permission-request-card__explanation')).toHaveCount(0);
  await page.screenshot({ path: '/private/tmp/hotsheet-permission-settings-wide.png', fullPage: true });

  await settings.getByRole('button', { name: 'Reset' }).click();
  await expect(presentation).toHaveJSProperty('value', 'popup');
  await expect(variant).toHaveJSProperty('value', 'pending');
  await expect(request).toHaveJSProperty('value', 'edit');
  await expect(automation).toHaveJSProperty('value', 'allow');
  await expect(alwaysSupported).toHaveJSProperty('checked', true);
  await expect(explanation).toHaveJSProperty('checked', true);
  await expect(page.locator('[data-component="permission-request-popup"]')).toBeVisible();
  await expect(card).toContainText('Auto-allow in');
  await page.setViewportSize({ width: 760, height: 900 });
  await page.screenshot({ path: '/private/tmp/hs2-xrva64-permission-countdown-narrow.png', fullPage: true });
  await choose(variant, 'external');
  await expect(card).toContainText('Decision made outside Hot Sheet');
  await expect(settings).toBeVisible();
  await expect(card).toBeVisible();
  await expect(page.getByRole('button', { name: 'Close settings' })).toBeVisible();
  const [cardBox, settingsBox] = await Promise.all([card.boundingBox(), settings.boundingBox()]);
  expect(cardBox!.x + cardBox!.width).toBeLessThanOrEqual(settingsBox!.x);
  await page.screenshot({ path: '/private/tmp/hotsheet-permission-settings-narrow.png', fullPage: true });
});

test('previews AIConversation public states at wide and narrow sizes',async({page})=>{
  await page.setViewportSize({width:1280,height:800});await page.goto('/ux-demo?component=ai-conversation');const conversationHost=page.locator('[data-component="ai-conversation"]'),dialog=conversationHost.getByRole('dialog'),transcript=conversationHost.locator('.ai-conversation__transcript');await expect(dialog).toBeVisible();await expect(conversationHost).toContainText('Running the focused browser test');await expect(conversationHost.getByRole('button',{name:'Stop Codex'})).toBeVisible();await expect(conversationHost.locator('.ai-conversation__message--user .markdown-preview').first()).toHaveCSS('color','rgb(255, 255, 255)');await dialog.screenshot({path:'/private/tmp/hs2-s3j29e-ai-conversation-demo-wide.png'});
  await conversationHost.getByRole('button',{name:'Close conversation'}).click();await expect(dialog).toBeHidden();await page.locator('[data-action="toggle-settings"]').click();const scenario=page.locator('[data-settings="ai-conversation"] [name="scenario"]');for(const value of ['empty','permission','completed','usage-unpriced','failed','interrupted'] as const){await scenario.evaluate((node:HTMLElement&{value:string},next)=>{node.value=next;node.dispatchEvent(new Event('change',{bubbles:true}))},value);await expect(dialog).toBeVisible();if(value==='empty')await expect(conversationHost).toContainText('Start a conversation');if(value==='permission')await expect(conversationHost.locator('[data-component="permission-request-card"]')).toBeVisible();if(value==='completed'){await expect(conversationHost).toContainText('≈$0.04');await expect(conversationHost).toContainText('AI-generated · may contain errors')}if(value==='usage-unpriced')await expect(conversationHost).toContainText('Cost unavailable');if(value==='failed'){await expect(conversationHost).toContainText('Conversation unavailable');await expect(conversationHost).toContainText('The tool turn failed.');await transcript.evaluate(node=>{node.scrollTop=node.scrollHeight});await expect(conversationHost.getByRole('alert')).toBeInViewport();await dialog.screenshot({path:'/private/tmp/hs2-1kqjbk-ai-conversation-failed-wide.png'})}if(value==='interrupted')await expect(conversationHost).toContainText('Stopped before the suite completed.');await conversationHost.getByRole('button',{name:'Close conversation'}).click();await expect(dialog).toBeHidden()}
  await page.setViewportSize({width:760,height:640});await scenario.evaluate((node:HTMLElement&{value:string})=>{node.value='failed';node.dispatchEvent(new Event('change',{bubbles:true}))});await expect(dialog).toBeVisible();await transcript.evaluate(node=>{node.scrollTop=node.scrollHeight});await expect(conversationHost.getByRole('alert')).toBeInViewport();await dialog.screenshot({path:'/private/tmp/hs2-1kqjbk-ai-conversation-failed-narrow.png'});
});
