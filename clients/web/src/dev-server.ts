import { readFile, stat } from 'node:fs/promises';
import { resolve } from 'node:path';

import { Hono } from 'hono';

import { createCliDevReviewSubmitter, type DevReviewSubmitter, validateDevReviewSubmission } from './dev-review/server';
import { openLocalProject, proxyProjectRequest, revealCorruptTicket } from './project-bridge';

export function createDevApp(dev = true, submitFeedback?: DevReviewSubmitter, reveal = revealCorruptTicket): Hono {
  const app = new Hono();
  app.post('/__hotsheet/projects/open', async context => {
    if (!dev) return context.notFound();
    try {
      const body = await context.req.json<{root:string;ticketStore?:string}>();
      return context.json(await openLocalProject(body.root, body.ticketStore), 201);
    } catch (error) {
      return context.json({ error: error instanceof Error ? error.message : 'Could not open project.' }, 400);
    }
  });
  app.all('/__hotsheet/project-api/:project/*', async context => {
    if (!dev) return context.notFound();
    const prefix = `/__hotsheet/project-api/${encodeURIComponent(context.req.param('project'))}`;
    const path = context.req.path.slice(prefix.length) || '/';
    return proxyProjectRequest(context.req.param('project'), `${path}${new URL(context.req.url).search}`, context.req.raw);
  });
  app.post('/__hotsheet/projects/:project/corrupt-tickets/reveal', async context => {
    if (!dev) return context.notFound();
    try {
      const body = await context.req.json<{path:string}>();
      await reveal(context.req.param('project'), body.path);
      return context.json({ revealed: true });
    } catch (error) {
      return context.json({ error: error instanceof Error ? error.message : 'Could not show the corrupt ticket file.' }, 400);
    }
  });
  app.get('/ux-demo', (context) => {
    if (!dev) return context.notFound();
    return context.html(`<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <meta name="robots" content="noindex,nofollow" />
    <title>Hot Sheet UX components</title>
  </head>
  <body>
    <div id="ux-demo"></div>
    <script type="module" src="/src/ux-demo/main.tsx"></script>
  </body>
</html>`);
  });
  app.get('/__hotsheet/demo-modified', async context => {
    if (!dev) return context.notFound();
    return context.json(await demoModifiedTimes(resolve(process.cwd(), 'src')));
  });
  app.post('/__hotsheet/dev-review/tickets', async context => {
    if (!dev || context.req.header('x-hotsheet-dev-review') !== '1') return context.notFound();
    try {
      const submission = validateDevReviewSubmission(await context.req.json());
      const submit = submitFeedback ?? createCliDevReviewSubmitter({
        repoRoot: process.env.HOTSHEET_DEV_REVIEW_REPO_ROOT ?? resolve(process.cwd(), '../..'),
        storePath: process.env.HOTSHEET_DEV_REVIEW_STORE,
        cliPath: process.env.HOTSHEET_DEV_REVIEW_CLI,
      });
      return context.json(await submit(submission), 201);
    } catch (error) {
      return context.json({ error: error instanceof Error ? error.message : 'Ticket creation failed.' }, 400);
    }
  });
  return app;
}

const demoEntries: Record<string, string> = {
  'app-shell': 'ux-demo/app-shell-demo.tsx', 'project-tab': 'ux-demo/app-shell-demo.tsx', 'project-tabs': 'ux-demo/app-shell-demo.tsx', 'resizable-region': 'ux-demo/app-shell-demo.tsx', 'connection-state-banner': 'ux-demo/app-shell-demo.tsx',
  'project-sidebar': 'ux-demo/project-sidebar-demo.tsx', 'project-summary': 'ux-demo/project-sidebar-demo.tsx', 'repository-summary': 'ux-demo/project-sidebar-demo.tsx', 'view-navigation': 'ux-demo/project-sidebar-demo.tsx', 'command-navigation': 'ux-demo/project-sidebar-demo.tsx', 'drive-control': 'ux-demo/project-sidebar-demo.tsx',
  'workspace-header': 'ux-demo/workspace-components-demo.tsx', 'page-header': 'ux-demo/workspace-components-demo.tsx', 'quick-ticket-composer': 'ux-demo/workspace-components-demo.tsx', 'ticket-inspector': 'ux-demo/workspace-components-demo.tsx',
  'ticket-list': 'ux-demo/ticket-collections-demo.tsx', 'ticket-row': 'ux-demo/ticket-row-demo.tsx', 'ticket-board': 'ux-demo/ticket-collections-demo.tsx', 'ticket-board-column': 'ux-demo/ticket-collections-demo.tsx',
  'ticket-info-panel': 'ux-demo/ticket-metadata-demo.tsx', 'ticket-timeline': 'ux-demo/ticket-metadata-demo.tsx', 'ticket-attachments': 'ux-demo/ticket-metadata-demo.tsx', 'ticket-category-select': 'ux-demo/ticket-metadata-demo.tsx', 'ticket-priority-select': 'ux-demo/ticket-metadata-demo.tsx', 'ticket-status-menu': 'ux-demo/ticket-metadata-demo.tsx',
  'ticket-reader': 'ux-demo/content-components-demo.tsx', 'markdown-editor': 'ux-demo/content-components-demo.tsx', 'note-card': 'ux-demo/content-components-demo.tsx', 'note-composer': 'ux-demo/content-components-demo.tsx', 'tag-chip': 'ux-demo/tag-chip-demo.tsx', 'status-badge': 'ux-demo/status-badge-demo.tsx',
  'select': 'ux-demo/select-demo.tsx', 'toolbar': 'ux-demo/toolbar-demo.tsx', 'menu-item': 'ux-demo/menu-item-demo.tsx', 'menu-header': 'ux-demo/menu-header-demo.tsx', 'toolbar-control-group': 'ux-demo/toolbar-control-group-demo.tsx', 'toolbar-text': 'ux-demo/toolbar-text-demo.tsx',
};

async function demoModifiedTimes(sourceRoot: string): Promise<Record<string, string>> {
  const resolveImport = async (from: string, specifier: string): Promise<string | undefined> => {
    const base = resolve(from, '..', specifier);
    for (const suffix of ['', '.ts', '.tsx', '.css', '/index.ts', '/index.tsx']) {
      const candidate = `${base}${suffix}`;
      try { if ((await stat(candidate)).isFile()) return candidate; } catch { /* try next extension */ }
    }
  };
  const dependencyTime = async (entry: string): Promise<number> => {
    const pending = [resolve(sourceRoot, entry)]; const seen = new Set<string>(); let newest = 0;
    while (pending.length) {
      const file = pending.pop()!; if (seen.has(file)) continue; seen.add(file);
      try {
        newest = Math.max(newest, (await stat(file)).mtimeMs);
        const source = await readFile(file, 'utf8');
        for (const match of source.matchAll(/(?:from\s*|import\s*)['"](\.{1,2}\/[^'"]+)['"]/g)) {
          const dependency = await resolveImport(file, match[1]); if (dependency) pending.push(dependency);
        }
      } catch { /* a removed optional dependency contributes no timestamp */ }
    }
    for (const shared of ['ux-demo/main.tsx', 'ux-demo/style.css']) newest = Math.max(newest, (await stat(resolve(sourceRoot, shared))).mtimeMs);
    return newest;
  };
  return Object.fromEntries(await Promise.all(Object.entries(demoEntries).map(async ([id, entry]) => [id, new Date(await dependencyTime(entry)).toISOString()])));
}

// This entry is loaded only by Vite's `serve` command (see vite.config.ts). It is
// deliberately absent from the production build graph.
export default createDevApp(true);
