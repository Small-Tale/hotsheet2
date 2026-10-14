import '@awesome.me/webawesome/dist/components/input/input.js';
import '@awesome.me/webawesome/dist/styles/utilities/visually-hidden.css';
import './lucide-icon-picker.css';

import { uiColor } from '@kerfjs/ui/css-values';
import { List } from '@kerfjs/ui/list';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Text } from '@kerfjs/ui/text';
import { Search } from 'lucide';

import {
  isLucideCatalogLoaded,
  type LucideCatalogEntry,
  lucideCatalogVersion,
  searchLucideCatalog,
} from '../lucide-catalog';
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
    : POPULAR_LUCIDE_ICONS.map((entry) => ({
        name: entry.name,
        icon: entry.icon,
        label: entry.name.replace(/-/g, ' '),
      }));
  const current = value ? resolveCommandIcon(value) : undefined;
  const selectedName = current?.name;
  const showCurrent = current && !trimmed && !results.some((entry) => entry.name === current.name);
  return (
    <div class="lucide-icon-picker" data-component="lucide-icon-picker">
      <List gap="xs">
        <wa-input
          class="wa-visually-hidden-label"
          label="Search Icons"
          type="search"
          size="small"
          name={searchName}
          value={query}
          placeholder="Search all icons…"
          autocomplete="off"
          spellcheck="false"
        >
          <LucideIcon slot="start" size="s" icon={Search} name="search" color={uiColor('neutral-on-quiet')} />
        </wa-input>
        {trimmed && !loaded ? (
          <Text size="compact" tone="quiet" role="status">
            Loading icons…
          </Text>
        ) : trimmed && results.length === 0 ? (
          <Text size="compact" tone="quiet">{`No icons match “${trimmed}”.`}</Text>
        ) : (
          <ul class="lucide-icon-picker__grid" aria-label="Icons">
            {showCurrent && (
              <li>
                <button
                  type="button"
                  class="lucide-icon-picker__icon"
                  data-action={selectAction}
                  data-icon-name={current.name}
                  aria-pressed="true"
                  aria-label={current.name}
                  title={current.name}
                >
                  <LucideIcon size={18} icon={current.icon} name={current.name} />
                </button>
              </li>
            )}
            {results.map((entry) => (
              <li>
                <button
                  type="button"
                  class="lucide-icon-picker__icon"
                  data-action={selectAction}
                  data-icon-name={entry.name}
                  aria-pressed={entry.name === selectedName ? 'true' : undefined}
                  aria-label={entry.name}
                  title={entry.name}
                >
                  <LucideIcon size={18} icon={entry.icon} name={entry.name} />
                </button>
              </li>
            ))}
          </ul>
        )}
      </List>
    </div>
  );
}
