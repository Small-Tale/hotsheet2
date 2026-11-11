import type { ResizeCommit } from '@kerfjs/ui/wire-resizable-regions';

type RegionCommit = (size: number) => void;

/**
 * The application wires `wireResizableRegions` once at its root (its handlers are delegated, so a
 * second wiring of the same root would resize twice). Each feature that owns a standalone
 * ResizableRegion registers its commit here by region id (HS2-BS0XFK).
 */
export function createRegionCommitRoutes() {
  const routes = new Map<string, RegionCommit>();
  return {
    route(id: string, commit: RegionCommit): () => void {
      routes.set(id, commit);
      return () => {
        if (routes.get(id) === commit) routes.delete(id);
      };
    },
    /** An arrow property, so the root wiring can take it unbound. */
    commit: (change: ResizeCommit): void => {
      routes.get(change.id)?.(change.size);
    },
  };
}

/** The application's routes, shared by the root wiring in `main.tsx` and the features it hosts. */
export const regionCommitRoutes = createRegionCommitRoutes();
