import { expect, type Page, test } from '@playwright/test';

const projects = {
  source: { id: 'source-project', root: '/work/source', name: 'Kerf', stores: ['/work/source.hs2'], apiPath: '/__hotsheet/project-api/source-project' },
  target: { id: 'target-project', root: '/work/target', name: 'Hot Sheet 2', stores: ['/work/target.hs2'], apiPath: '/__hotsheet/project-api/target-project' },
  deep: { id: 'third-project', root: '/work/deep', name: 'Deep Work', stores: ['/work/deep.hs2'], apiPath: '/__hotsheet/project-api/third-project' },
} as const;
const base = { category: 'bug', priority: 'default', status: 'started', up_next: false, feedback_needed: false, tags: [], blocked_by: [], claim_count: 0, created_at: '2026-09-11T00:00:00Z', updated_at: '2026-09-11T00:01:00Z' };
const source = { ...base, connection_id: 'git-source', native_id: 'source', qualified_id: 'git-source:source', id: 'source', slug: 'KF-ROOT01', title: 'Original workspace selection', details: 'Open @target-project/HS2-LINK01 without leaving this ticket.' };
const target = { ...base, connection_id: 'git-target', native_id: 'target', qualified_id: 'git-target:target', id: 'target', slug: 'HS2-LINK01', title: 'First linked reader', details: 'Continue to @third-project/HS2-LINK01 in another project.' };
const deep = { ...base, connection_id: 'git-deep', native_id: 'deep', qualified_id: 'git-deep:deep', id: 'deep', slug: 'HS2-LINK01', title: 'Same slug, different project', details: 'The navigation stack can now unwind safely.' };
type FixtureTicket = typeof source | typeof target | typeof deep;
const rows: Record<string, FixtureTicket[]> = { 'source-project': [source], 'target-project': [target], 'third-project': [deep] };
const projectByRoot = new Map<string, (typeof projects)[keyof typeof projects]>(Object.values(projects).map(project => [project.root, project]));

async function mockLayeredProjects(page: Page, mutations: Array<{projectId:string;ticketId:string;patch:Record<string,unknown>}>=[]) {
  const chooser = ['/work/target', '/work/deep'];
  await page.route('**/*', async route => {
    const request = route.request(), url = new URL(request.url()), path = decodeURIComponent(url.pathname);
    if (path === '/__hotsheet/projects/open') {
      const root = (request.postDataJSON() as { root: string }).root;
      return route.fulfill({ status: 201, json: projectByRoot.get(root) ?? projects.source });
    }
    if (path === '/__hotsheet/folders/choose') return route.fulfill({ json: { path: chooser.shift() } });
    const projectId = path.match(/project-api\/([^/]+)/)?.[1], project = projectId ? Object.values(projects).find(item => item.id === projectId) : undefined, tickets = projectId ? rows[projectId] ?? [] : [];
    if (path.endsWith('/providers')) return route.fulfill({ json: [{ connection_id: tickets[0]?.connection_id ?? 'git-source', provider: 'git', display_name: `${project?.name ?? 'Project'} git`, locator: project?.stores[0] ?? '', default: true, capabilities: { create: true, update: true, close: true, notes: true, note_edit: true, note_delete: true, attachments: true, assignment: true, review_requests: true, dependencies: true, up_next: true, close_reasons: true, claims: true, atomic_batch: true, not_working_report: true, offline_mutation: true, history: true, watch: true, provider_idempotency: true, query_fields: [] } }] });
    if (path.endsWith('/provider-connections') || path.endsWith('/corrupt-tickets') || path.endsWith('/commands') || path.endsWith('/terminals') || path.endsWith('/permissions') || path.endsWith('/connections') || path.endsWith('/drive/sessions')) return route.fulfill({ json: [] });
    if (path.endsWith('/repository/status')) return route.fulfill({ json: { branch: 'main', ahead: 0, behind: 0, staged: 0, unstaged: 0, untracked: 0, conflicted: 0, clean: true } });
    if (path.endsWith('/terminal-settings')) return route.fulfill({ json: { inherit_global_shell_history: false } });
    if (path.endsWith('/ws/poll')) { await new Promise(resolve => setTimeout(resolve, 500)); return route.fulfill({ json: { cursor: 0, events: [], overflow: false } }); }
    if (path.endsWith('/tickets') && request.method() === 'GET') {
      const query = url.searchParams.get('text')?.toLocaleLowerCase();
      return route.fulfill({ json: query ? tickets.filter(ticket => ticket.slug.toLocaleLowerCase() === query || ticket.title.toLocaleLowerCase().includes(query)) : tickets });
    }
    const ticketId = path.match(/\/tickets\/([^/]+)$/)?.[1];
    if (ticketId && request.method() === 'PATCH') {
      const ticket = tickets.find(item => [item.id, item.native_id, item.qualified_id].some(identity => identity === ticketId));
      const patch=request.postDataJSON() as Record<string,unknown>;
      if(!ticket)return route.fulfill({status:404,json:{error:`No ${ticketId}`}});
      mutations.push({projectId:projectId!,ticketId,patch});
      return route.fulfill({json:{store:ticket.connection_id,...ticket,...patch,blocked_reason:patch.blocked_reason??null,concurrency_token:`token-${ticket.id}-${mutations.length}`,notes:[],attachments:[]}});
    }
    if (ticketId && request.method() === 'GET') {
      const ticket = tickets.find(item => [item.id, item.native_id, item.qualified_id].some(identity => identity === ticketId));
      return ticket ? route.fulfill({ json: { store: ticket.connection_id, ...ticket, blocked_reason: null, concurrency_token: `token-${ticket.id}`, notes: [], attachments: [] } }) : route.fulfill({ status: 404, json: { error: `No ${ticketId}` } });
    }
    if (!path.startsWith('/__hotsheet/')) return route.continue();
    return route.fulfill({ status: 404, json: { error: `Unhandled ${request.method()} ${path}` } });
  });
}

async function openProjects(page: Page) {
  await page.goto('/?dev-review=false');
  await page.getByRole('button', { name: 'Open project' }).click();
  await page.getByRole('button', { name: 'Open project', exact: true }).last().click();
  await page.getByRole('button', { name: 'Add project' }).click();
  await expect(page.getByRole('tab', { name: 'Hot Sheet 2' })).toHaveAttribute('aria-selected', 'true');
  await page.getByRole('button', { name: 'Add project' }).click();
  await expect(page.getByRole('tab', { name: 'Deep Work' })).toHaveAttribute('aria-selected', 'true');
  await page.getByRole('tab', { name: 'Kerf' }).click();
  await page.locator('[data-ticket-slug="KF-ROOT01"]').click();
}

test('layers exact cross-project ticket readers and unwinds focus without changing workspace selection', async ({ page }) => {
  await mockLayeredProjects(page);
  await page.setViewportSize({ width: 1200, height: 900 });
  await openProjects(page);
  const workspaceInspector = page.locator('[data-component="ticket-inspector"][data-presentation="sidebar"]');
  const inspectorLink = workspaceInspector.getByRole('link', { name: '@target-project/HS2-LINK01' });
  await inspectorLink.click();
  const first = page.getByRole('dialog', { name: 'Read and edit HS2-LINK01 in Hot Sheet 2' });
  await expect(first).toBeVisible();
  await expect(page.getByRole('tab', { name: 'Kerf' })).toHaveAttribute('aria-selected', 'true');
  await expect(workspaceInspector).toHaveAttribute('data-ticket-slug', 'KF-ROOT01');
  await first.getByRole('link', { name: '@third-project/HS2-LINK01' }).click();
  const third = page.getByRole('dialog', { name: 'Read and edit HS2-LINK01 in Deep Work' });
  await expect(third).toBeVisible();
  await expect(third.getByText('Reader 2 of 2')).toBeVisible();
  await page.screenshot({ path: '/private/tmp/hs2-t5vnj8-layered-readers-wide.png', fullPage: true });
  await page.setViewportSize({ width: 720, height: 760 });
  await expect(third).toBeVisible();
  await page.screenshot({ path: '/private/tmp/hs2-t5vnj8-layered-readers-narrow.png', fullPage: true });
  await page.keyboard.press('Escape');
  await expect(third).toHaveCount(0);
  await expect(first.getByRole('link', { name: '@third-project/HS2-LINK01' })).toBeFocused();
  await first.getByRole('button', { name: 'Close ticket reader' }).click();
  await expect(first).toHaveCount(0);
  await expect(inspectorLink).toBeFocused();
  await expect(page.getByRole('tab', { name: 'Kerf' })).toHaveAttribute('aria-selected', 'true');
  await expect(workspaceInspector).toHaveAttribute('data-ticket-slug', 'KF-ROOT01');
});

test('layers links above an existing reader and closes one level at a time', async ({ page }) => {
  await mockLayeredProjects(page);
  await openProjects(page);
  await page.locator('[data-component="ticket-list-row"][data-ticket-slug="KF-ROOT01"]').dblclick();
  const baseReader = page.getByRole('dialog', { name: 'Read and edit KF-ROOT01 in Kerf' });
  await baseReader.getByRole('link', { name: '@target-project/HS2-LINK01' }).click();
  const linked = page.getByRole('dialog', { name: 'Read and edit HS2-LINK01 in Hot Sheet 2' });
  await expect(linked.getByText('Reader 2 of 2')).toBeVisible();
  await expect(baseReader).not.toBeVisible();
  await linked.getByRole('button', { name: 'Close ticket reader' }).click();
  await expect(linked).toHaveCount(0);
  await expect(baseReader).toBeVisible();
  await expect(baseReader.getByRole('link', { name: '@target-project/HS2-LINK01' })).toBeFocused();
});

test('edits same-slug linked readers through their owning project and flushes before unwind', async ({ page }) => {
  const mutations:Array<{projectId:string;ticketId:string;patch:Record<string,unknown>}>=[];
  await mockLayeredProjects(page,mutations);
  await page.setViewportSize({width:1200,height:900});
  await openProjects(page);
  await page.locator('[data-component="ticket-inspector"][data-presentation="sidebar"]').getByRole('link',{name:'@target-project/HS2-LINK01'}).click();
  const targetReader=page.getByRole('dialog',{name:'Read and edit HS2-LINK01 in Hot Sheet 2'});
  await targetReader.getByRole('link',{name:'@third-project/HS2-LINK01'}).click();
  const deepReader=page.getByRole('dialog',{name:'Read and edit HS2-LINK01 in Deep Work'});
  await deepReader.locator('[data-action="edit-markdown"]').dblclick();
  await deepReader.getByRole('textbox',{name:'Ticket details'}).fill('Deep project draft stays with git-deep.');
  await page.screenshot({path:'/private/tmp/hs2-1xqb2k-linked-edit-wide.png',fullPage:true});
  await page.setViewportSize({width:720,height:760});
  await page.screenshot({path:'/private/tmp/hs2-1xqb2k-linked-edit-narrow.png',fullPage:true});
  await deepReader.getByRole('button',{name:'Close ticket reader'}).click();
  await expect(deepReader).toHaveCount(0);
  await targetReader.locator('[data-action="edit-markdown"]').dblclick();
  await targetReader.getByRole('textbox',{name:'Ticket details'}).fill('Target project draft stays with git-target.');
  await targetReader.getByRole('button',{name:'Close ticket reader'}).click();
  await expect(targetReader).toHaveCount(0);
  await expect.poll(()=>mutations.map(item=>[item.projectId,item.ticketId,item.patch.details])).toEqual([
    ['third-project','deep','Deep project draft stays with git-deep.'],
    ['target-project','target','Target project draft stays with git-target.'],
  ]);
  await expect(page.locator('[data-component="ticket-inspector"][data-presentation="sidebar"]')).toHaveAttribute('data-ticket-slug','KF-ROOT01');
});
