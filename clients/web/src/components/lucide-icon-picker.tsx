import './lucide-icon-picker.css';

import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Search } from 'lucide';

import { isLucideCatalogLoaded, type LucideCatalogEntry, lucideCatalogVersion, searchLucideCatalog } from '../lucide-catalog';
import { resolveCommandIcon } from './command-icon';
import { POPULAR_LUCIDE_ICONS } from './lucide-popular';

export interface LucideIconPickerProps {
  /** The selected icon's kebab-case name. */
  value?: string;
  /** The current search query (controlled by the host). */
  query?: string;
  /** `input` element name for the search field, delegated by the host. */
  searchName: string;
  /** `data-action` set on each icon button (with `data-icon-name`), delegated by the host. */
  selectAction: string;
}

/**
 * A searchable icon picker: curated popular icons by default, and any Lucide icon once a query is
 * entered (the full catalog loads lazily). Reads {@link lucideCatalogVersion} so it re-renders when
 * the catalog finishes loading.
 */
export function LucideIconPicker({ value, query = '', searchName, selectAction }: LucideIconPickerProps) {
  const loaded = isLucideCatalogLoaded() && lucideCatalogVersion.value >= 0;
  const trimmed = query.trim();
  const results: LucideCatalogEntry[] = trimmed
    ? searchLucideCatalog(query)
    : POPULAR_LUCIDE_ICONS.map(entry => ({ name: entry.name, icon: entry.icon, label: entry.name.replace(/-/g, ' ') }));
  const current = value ? resolveCommandIcon(value) : undefined;
  const selectedName = current?.name;
  const showCurrent = current && !trimmed && !results.some(entry => entry.name === current.name);
  return <div class="lucide-icon-picker" data-component="lucide-icon-picker">
    <label class="lucide-icon-picker__search"><span class="lucide-icon-picker__search-icon" aria-hidden="true"><LucideIcon icon={Search} name="search"/></span><input type="search" name={searchName} value={query} placeholder="Search all icons…" aria-label="Search icons" autocomplete="off" spellcheck="false"/></label>
    {trimmed && !loaded
      ? <p class="lucide-icon-picker__hint" role="status">Loading icons…</p>
      : <ul class="lucide-icon-picker__grid" aria-label="Icons">
          {showCurrent && <li><button type="button" class="lucide-icon-picker__icon" data-action={selectAction} data-icon-name={current.name} aria-pressed="true" aria-label={current.name} title={current.name}><LucideIcon icon={current.icon} name={current.name}/></button></li>}
          {results.map(entry => <li><button type="button" class="lucide-icon-picker__icon" data-action={selectAction} data-icon-name={entry.name} aria-pressed={entry.name === selectedName ? 'true' : undefined} aria-label={entry.name} title={entry.name}><LucideIcon icon={entry.icon} name={entry.name}/></button></li>)}
          {trimmed && results.length === 0 && <li class="lucide-icon-picker__empty">No icons match “{trimmed}”.</li>}
        </ul>}
  </div>;
}
