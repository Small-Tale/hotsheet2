import '@awesome.me/webawesome/dist/components/input/input.js';
import '@awesome.me/webawesome/dist/styles/utilities/visually-hidden.css';
import './lucide-icon-picker.css';

import { ChoiceChips } from '@kerfjs/ui/choice-chips';
import { px, uiColor } from '@kerfjs/ui/css-values';
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
  /** `data-action` set on each native icon radio, delegated by the host on change. */
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
  const choices = [
    ...(showCurrent ? [{ name: current.name, icon: current.icon, label: current.name.replace(/-/g, ' ') }] : []),
    ...results,
  ].map((entry) => ({
    value: entry.name,
    label: entry.label,
    icon: <LucideIcon size={18} icon={entry.icon} name={entry.name} />,
    title: entry.name,
  }));
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
          <ChoiceChips
            name="command-icon-choice"
            label="Icons"
            presentation="icons"
            size="compact"
            maxHeight={px(184)}
            choices={choices}
            value={selectedName}
            action={selectAction}
          />
        )}
      </List>
    </div>
  );
}
