import { Hono } from 'hono';

export function createDevApp(dev = true): Hono {
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
  return app;
}

// This entry is loaded only by Vite's `serve` command (see vite.config.ts). It is
// deliberately absent from the production build graph.
export default createDevApp(true);
