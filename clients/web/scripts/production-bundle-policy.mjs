export const MAX_INITIAL_ASSETS = 4;

export function initialAssetPaths(html) {
  return [...new Set([...html.matchAll(/(?:src|href)=["']([^"']+)["']/g)]
    .map(match => match[1])
    .filter(path => path.startsWith('/assets/')))];
}

export function assertInitialAssetBudget(html, maximum = MAX_INITIAL_ASSETS) {
  const assets = initialAssetPaths(html);
  if (assets.length > maximum) {
    throw new Error(`Production entry point loads ${assets.length} assets; budget is ${maximum}: ${assets.join(', ')}`);
  }
  return assets;
}
