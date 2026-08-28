import { Hono } from 'hono';
import { resolve } from 'node:path';
import { createCliDevReviewSubmitter, type DevReviewSubmitter, validateDevReviewSubmission } from './dev-review/server';

export function createDevApp(dev = true, submitFeedback?: DevReviewSubmitter): Hono {
  const app = new Hono();
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
  app.post('/__hotsheet/dev-review/tickets', async context => {
    if (!dev || context.req.header('x-hotsheet-dev-review') !== '1') return context.notFound();
    try {
      const submission = validateDevReviewSubmission(await context.req.json());
      const submit = submitFeedback ?? createCliDevReviewSubmitter({
        repoRoot: resolve(process.cwd(), '../..'),
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

// This entry is loaded only by Vite's `serve` command (see vite.config.ts). It is
// deliberately absent from the production build graph.
export default createDevApp(true);
