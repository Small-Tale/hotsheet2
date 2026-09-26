import { existsSync, readFileSync, statSync } from 'node:fs';
import { dirname, resolve } from 'node:path';

/** Browser entry modules whose import graphs the frozen stable-dev server pre-bundles (HS2-N9RD7X). */
export const STABLE_DEV_BROWSER_ENTRIES = ['src/main.tsx', 'src/ux-demo/main.tsx'] as const;

const SOURCE_EXTENSIONS = ['.ts', '.tsx', '.js', '.mjs', '/index.ts', '/index.tsx'];
// Static imports/re-exports (`import x from`, `import 'x'`, `export … from`) and dynamic `import('x')`.
// `import type` / `export type` are erased at build time and never reach the browser.
const IMPORT_PATTERN =
  /(?:^|[;\s])(?:import|export)\s+(?!type\s)(?:[\w*{}\s,$]+?\s+from\s+)?['"]([^'"]+)['"]|\bimport\(\s*['"]([^'"]+)['"]\s*\)/g;

function resolveSource(fromFile: string, specifier: string): string | undefined {
  const base = resolve(dirname(fromFile), specifier);
  if (existsSync(base) && statSync(base).isFile()) return base;
  for (const extension of SOURCE_EXTENSIONS) {
    const candidate = `${base}${extension}`;
    if (existsSync(candidate)) return candidate;
  }
  return undefined;
}

function bareDependency(specifier: string): boolean {
  if (specifier.startsWith('.') || specifier.startsWith('/') || specifier.startsWith('\0')) return false;
  if (/^(?:node|virtual|data|https?):/.test(specifier)) return false;
  // Stylesheets and query-suffixed assets are served by Vite's own pipeline, not esbuild pre-bundling.
  return !/\.(?:css|scss|svg|png|json)$/.test(specifier) && !specifier.includes('?');
}

/**
 * Walk the browser import graph from `entries` (relative to `root`) and return every bare package
 * specifier it reaches, including lazily imported ones, sorted. The frozen stable-dev server passes
 * these to `optimizeDeps.include` so dependencies are pre-bundled once at startup: runtime discovery
 * stays off (it re-optimized and reloaded open pages, HS2-ATE664), but a tab no longer fetches
 * thousands of raw `node_modules` files (HS2-N9RD7X).
 */
export function scanBrowserDependencies(
  root: string,
  entries: readonly string[] = STABLE_DEV_BROWSER_ENTRIES,
  extra: readonly string[] = ['kerfjs/jsx-runtime', 'kerfjs/jsx-dev-runtime'],
): string[] {
  const dependencies = new Set(extra);
  const visited = new Set<string>();
  const pending = entries.map((entry) => resolve(root, entry)).filter((file) => existsSync(file));
  while (pending.length) {
    const file = pending.pop()!;
    if (visited.has(file)) continue;
    visited.add(file);
    const source = readFileSync(file, 'utf8');
    for (const match of source.matchAll(IMPORT_PATTERN)) {
      // Group 1 is a static import or re-export; group 2 a dynamic import(). Unmatched groups are undefined.
      const specifier = match[1] || match[2];
      if (specifier.startsWith('.')) {
        const next = resolveSource(file, specifier);
        if (next && !visited.has(next)) pending.push(next);
      } else if (bareDependency(specifier)) dependencies.add(specifier);
    }
  }
  return [...dependencies].sort();
}
