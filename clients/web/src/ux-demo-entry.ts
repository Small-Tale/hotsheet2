/**
 * The production entry's `/ux-demo` branch (HS2-ZF70Y9). A host started with `--feature ux-demo`
 * answers `/ux-demo` with the application document; the entry then turns that document into the
 * catalog page the development server renders (`dev-server.ts`) and loads the catalog. The dynamic
 * import keeps the catalog in its own chunk, so the app never downloads it.
 */
export const UX_DEMO_ROOT_ID = 'ux-demo';
export const UX_DEMO_TITLE = 'Hot Sheet UX components';

/** Replace the app root with the catalog root, and title the page and keep it out of search indexes. */
export function prepareUxDemoDocument(document: Document): HTMLElement {
  const existing = document.getElementById(UX_DEMO_ROOT_ID);
  if (existing) return existing;
  const root = document.createElement('div');
  root.id = UX_DEMO_ROOT_ID;
  root.className = 'kui-app-root';
  const app = document.getElementById('app');
  if (app) app.replaceWith(root);
  else document.body.append(root);
  document.title = UX_DEMO_TITLE;
  if (!document.querySelector('meta[name="robots"]')) {
    const robots = document.createElement('meta');
    robots.name = 'robots';
    robots.content = 'noindex,nofollow';
    document.head.append(robots);
  }
  return root;
}

/** Prepare the document, then load the catalog, whose module mounts itself on the prepared root. */
export async function bootUxDemo(
  document: Document,
  load: () => Promise<unknown> = () => import('./ux-demo/main'),
): Promise<void> {
  prepareUxDemoDocument(document);
  await load();
}
