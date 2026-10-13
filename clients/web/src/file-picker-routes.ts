import { type FilePickerSelection, wireFilePickers } from '@kerfjs/ui/wire-file-pickers';

type Root = HTMLElement | Document;
type Handler = (files: File[], selection: FilePickerSelection) => void;

interface RootRoutes {
  readonly handlers: Map<string, Handler>;
  readonly dispose: () => void;
}

const routesByRoot = new WeakMap<Root, RootRoutes>();

/**
 * Route files a Kerf FilePickerTrigger or FileDropZone reports, by its `pickerId`, through one shared
 * `wireFilePickers` per root (HS2-RB7KXZ). Kerf clears a picker's input after reporting it, so two
 * independent wirings on the same root would let the first consume files meant for the second.
 * Empty reports are ignored. Returns a disposer that removes this route and, with the last route,
 * the root's wiring.
 */
export function routeFilePicker(root: Root, pickerId: string, handler: Handler): () => void {
  let routes = routesByRoot.get(root);
  if (!routes) {
    const handlers = new Map<string, Handler>();
    routes = {
      handlers,
      dispose: wireFilePickers(root, {
        onFiles: (selection) => {
          if (selection.id === undefined || !selection.files.length) return;
          handlers.get(selection.id)?.(selection.files, selection);
        },
      }),
    };
    routesByRoot.set(root, routes);
  }
  const current = routes;
  current.handlers.set(pickerId, handler);
  return () => {
    if (current.handlers.get(pickerId) !== handler) return;
    current.handlers.delete(pickerId);
    if (current.handlers.size) return;
    current.dispose();
    routesByRoot.delete(root);
  };
}
