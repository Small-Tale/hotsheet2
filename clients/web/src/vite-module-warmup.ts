/** The slice of a Vite environment (`server.environments.client`) that the warm-up crawl needs. */
export interface WarmableEnvironment {
  transformRequest(url: string): Promise<unknown>;
  moduleGraph: {
    getModuleByUrl(url: string): Promise<{ importedModules: Iterable<{ url: string }> } | undefined>;
  };
}

/**
 * Transform the client module graph reachable from `entry` once, static and dynamic imports alike,
 * and resolve with the number of modules visited. Vite caches each result on its module node, so
 * later browser requests for these modules skip the transform. A module that fails to transform is
 * skipped: the browser request that needs it reports the real error (HS2-15EYK1).
 */
export async function warmClientModuleGraph(environment: WarmableEnvironment, entry: string): Promise<number> {
  const seen = new Set<string>();
  const visit = async (url: string): Promise<void> => {
    if (seen.has(url)) return;
    seen.add(url);
    try {
      await environment.transformRequest(url);
    } catch {
      return;
    }
    const module = await environment.moduleGraph.getModuleByUrl(url);
    await Promise.all([...(module?.importedModules ?? [])].map((imported) => visit(imported.url)));
  };
  await visit(entry);
  return seen.size;
}
