import { signal } from 'kerfjs';
import type { IconNode } from 'lucide';

/** One entry in the searchable Lucide catalog: its kebab-case name, node, and a display label. */
export interface LucideCatalogEntry {
  name: string;
  icon: IconNode;
  label: string;
}

let catalog: Map<string, IconNode> | undefined;
let entries: LucideCatalogEntry[] | undefined;
let loading: Promise<void> | undefined;

/** Bumped when the catalog finishes loading so reactive renders can resolve custom icons. */
export const lucideCatalogVersion = signal(0);

/** Convert a Lucide PascalCase export name (e.g. `GitBranch`) to its kebab-case id (`git-branch`). */
export function lucidePascalToKebab(pascal: string): string {
  return pascal
    .replace(/([a-z0-9])([A-Z])/g, '$1-$2')
    .replace(/([A-Z]+)([A-Z][a-z])/g, '$1-$2')
    .toLowerCase();
}

/**
 * Lazily load the full Lucide icon set (a separate chunk, kept out of the main bundle) once, building
 * a name→node map and a sorted, de-duplicated entry list. Safe to call repeatedly.
 */
export function loadLucideCatalog(): Promise<void> {
  if (catalog) return Promise.resolve();
  loading ??= import('lucide').then(module => {
    const map = new Map<string, IconNode>();
    const seen = new Set<IconNode>();
    const list: LucideCatalogEntry[] = [];
    for (const [pascal, value] of Object.entries(module)) {
      if (!Array.isArray(value) || seen.has(value)) continue;
      seen.add(value);
      const name = lucidePascalToKebab(pascal);
      if (map.has(name)) continue;
      map.set(name, value);
      list.push({ name, icon: value, label: name.replace(/-/g, ' ') });
    }
    list.sort((left, right) => left.name.localeCompare(right.name));
    catalog = map;
    entries = list;
    lucideCatalogVersion.value += 1;
  }).catch(() => { loading = undefined; });
  return loading;
}

/** The resolved node for a kebab-case Lucide name, or undefined until the catalog has loaded. */
export function lucideIconNode(name: string): IconNode | undefined {
  return catalog?.get(name);
}

/** All catalog entries (empty until loaded), for the icon picker's searchable grid. */
export function lucideCatalogEntries(): readonly LucideCatalogEntry[] {
  return entries ?? [];
}

export function isLucideCatalogLoaded(): boolean {
  return catalog !== undefined;
}

/**
 * Filter the catalog by a query (matched against the kebab name), returning at most `limit` entries.
 * With no query, returns the first `limit` entries (callers usually show curated favourites instead).
 */
export function searchLucideCatalog(query: string, limit = 80): LucideCatalogEntry[] {
  const all = lucideCatalogEntries();
  const trimmed = query.trim().toLowerCase().replace(/\s+/g, '-');
  if (!trimmed) return all.slice(0, limit);
  const starts: LucideCatalogEntry[] = [];
  const contains: LucideCatalogEntry[] = [];
  for (const entry of all) {
    if (entry.name.startsWith(trimmed)) starts.push(entry);
    else if (entry.name.includes(trimmed)) contains.push(entry);
    if (starts.length >= limit) break;
  }
  return [...starts, ...contains].slice(0, limit);
}
