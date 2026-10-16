import { CatalogExample } from '@kerfjs/ui/catalog';
import { Select, type SelectChoice } from '@kerfjs/ui/select';
import { delegate, signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import { SearchScopeToolbar, wireSearchScopeToolbar } from '../components/search-scope-toolbar';
import type { DemoModule, FormControl } from './demo-module';
import { syncSettingsControls } from './settings-controls';

/** Searched views of each kind, including a long custom-view name. */
const VIEW_LABEL_CHOICES: SelectChoice[] = [
  { value: 'Queue', label: 'Queue' },
  { value: 'Backlog', label: 'Backlog' },
  { value: 'Archive', label: 'Archive' },
  { value: 'Needs docs review', label: 'Custom View (Long Name)' },
];

/** Every public SearchScopeToolbar state: either scope, from each kind of view a search may widen. */
export const searchScopeToolbarSettings = {
  scope: signal<'view' | 'all-tickets'>('view'),
  viewLabel: signal<string>('Queue'),
};

export function resetSearchScopeToolbarDemo(root?: ParentNode): void {
  searchScopeToolbarSettings.scope.value = 'view';
  searchScopeToolbarSettings.viewLabel.value = 'Queue';
  if (root)
    syncSettingsControls(root, 'search-scope-toolbar', {
      values: {
        scope: searchScopeToolbarSettings.scope.value,
        viewLabel: searchScopeToolbarSettings.viewLabel.value,
      },
    });
}

export function SearchScopeToolbarDemo() {
  return (
    <CatalogExample
      label="Over Search Results"
      note="Shown only while a search is active outside Trash. Choosing All Tickets searches every collection except Trash; choosing the view returns the search to it."
      viewport={{ width: 'full', minHeight: 'short', surface: 'lowered' }}
    >
      <SearchScopeToolbar
        allTickets={searchScopeToolbarSettings.scope.value === 'all-tickets'}
        viewLabel={searchScopeToolbarSettings.viewLabel.value}
      />
    </CatalogExample>
  );
}

export function SearchScopeToolbarSettings() {
  return (
    <form class="settings-form" data-settings="search-scope-toolbar">
      <Select
        name="scope"
        label="Scope"
        value={searchScopeToolbarSettings.scope.value}
        choices={[
          { value: 'view', label: 'Selected View' },
          { value: 'all-tickets', label: 'All Tickets' },
        ]}
      />
      <Select
        name="viewLabel"
        label="Searched View"
        value={searchScopeToolbarSettings.viewLabel.value}
        choices={VIEW_LABEL_CHOICES}
      />
      <wa-button type="button" data-action="reset-settings">
        Reset
      </wa-button>
    </form>
  );
}

/** Install this file's demo interactions on the catalog root: the production wiring drives a fixture. */
function wireSearchScopeToolbarDemo(root: HTMLElement): () => void {
  const demoListeners = createScope();
  demoListeners.add(
    wireSearchScopeToolbar(root, {
      setAllTickets: (allTickets) => {
        searchScopeToolbarSettings.scope.value = allTickets ? 'all-tickets' : 'view';
        syncSettingsControls(root, 'search-scope-toolbar', {
          values: { scope: searchScopeToolbarSettings.scope.value },
        });
      },
    }),
  );
  demoListeners.add(
    delegate(root, 'change', '[data-settings="search-scope-toolbar"] [name]', (_event, target) => {
      const control = target as FormControl;
      if (control.getAttribute('name') === 'scope')
        searchScopeToolbarSettings.scope.value = control.value as typeof searchScopeToolbarSettings.scope.value;
      if (control.getAttribute('name') === 'viewLabel') searchScopeToolbarSettings.viewLabel.value = control.value;
    }),
  );
  return () => {
    demoListeners.dispose();
  };
}

/** The catalog entries this file renders. */
export const demoModule: DemoModule = {
  entries: {
    'search-scope-toolbar': {
      catalogExamples: true,
      Demo: SearchScopeToolbarDemo,
      Settings: SearchScopeToolbarSettings,
      reset: resetSearchScopeToolbarDemo,
    },
  },
  wire: wireSearchScopeToolbarDemo,
};
