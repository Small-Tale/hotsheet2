import type { IconNode } from 'lucide';

import { lucideIconNode } from '../lucide-catalog';
import { POPULAR_LUCIDE_MAP } from './lucide-popular';

/** Old command-icon keys that were not the Lucide kebab name, mapped to their Lucide id. */
const LEGACY_ICON_ALIASES: Record<string, string> = { test: 'test-tube-2', build: 'hammer' };

const FALLBACK_ICON_NAME = 'send';

/** The canonical Lucide id for a stored command icon (mapping legacy keys), defaulting to `send`. */
export function commandIconName(icon?: string): string {
  if (!icon) return FALLBACK_ICON_NAME;
  return LEGACY_ICON_ALIASES[icon] ?? icon;
}

/**
 * Resolve a command's stored icon to a renderable node: bundled popular icons resolve synchronously;
 * others resolve from the lazily loaded full catalog once available, falling back to a default until
 * then. Reactive callers read {@link lucideCatalogVersion} so they re-render when the catalog loads.
 */
export function resolveCommandIcon(icon?: string): { name: string; icon: IconNode } {
  const name = commandIconName(icon);
  const node = POPULAR_LUCIDE_MAP.get(name) ?? lucideIconNode(name) ?? POPULAR_LUCIDE_MAP.get(FALLBACK_ICON_NAME)!;
  return { name, icon: node };
}

/** True when a stored icon needs the full lazy catalog (it is neither empty nor a bundled popular icon). */
export function commandIconNeedsCatalog(icon?: string): boolean {
  const name = commandIconName(icon);
  return name !== FALLBACK_ICON_NAME && !POPULAR_LUCIDE_MAP.has(name);
}
