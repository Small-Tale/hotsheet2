export const MAX_INITIAL_ASSETS = 4;

export function initialAssetPaths(html) {
  return [
    ...new Set(
      [...html.matchAll(/(?:src|href)=["']([^"']+)["']/g)]
        .map((match) => match[1])
        .filter((path) => path.startsWith('/assets/')),
    ),
  ];
}

export function assertInitialAssetBudget(html, maximum = MAX_INITIAL_ASSETS) {
  const assets = initialAssetPaths(html);
  if (assets.length > maximum) {
    throw new Error(`Production entry point loads ${assets.length} assets; budget is ${maximum}: ${assets.join(', ')}`);
  }
  return assets;
}

/** Source paths of the component catalog, as a production source map lists them. */
const UX_DEMO_SOURCE = /(?:^|\/)src\/ux-demo\//;
/** The catalog's own entry module, which the production entry imports dynamically (HS2-ZF70Y9). */
const UX_DEMO_ENTRY = /(?:^|\/)src\/ux-demo\/main\.tsx$/;

/**
 * The production client ships the `/ux-demo` catalog only as an on-demand chunk (HS2-ZF70Y9): no
 * script the document loads at startup may contain catalog modules, and some other chunk must
 * contain the catalog's entry. `chunks` maps each emitted script's file name to its source map's
 * `sources`. Returns the chunks that contain the catalog entry.
 */
export function assertUxDemoIsLazy(initialScripts, chunks) {
  for (const script of initialScripts) {
    const leaked = (chunks[script] ?? []).filter((source) => UX_DEMO_SOURCE.test(source));
    if (leaked.length > 0)
      throw new Error(
        `The startup script ${script} contains UX demo catalog modules: ${leaked.slice(0, 5).join(', ')}`,
      );
  }
  const lazy = Object.entries(chunks)
    .filter(([, sources]) => sources.some((source) => UX_DEMO_ENTRY.test(source)))
    .map(([name]) => name);
  if (lazy.length === 0)
    throw new Error('No production chunk contains the UX demo catalog entry (src/ux-demo/main.tsx).');
  return lazy;
}
