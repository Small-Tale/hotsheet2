import '@kerfjs/ui/floating-toolbar.css';
import './search-scope-toolbar.css';

import { FloatingToolbar } from '@kerfjs/ui/floating-toolbar';
import { SegmentedControl } from '@kerfjs/ui/segmented-control';
import { ToolbarControlGroup } from '@kerfjs/ui/toolbar-control-group';
import { delegate } from 'kerfjs';
import { action } from 'kerfjs/actions';

/** `data-action` spec for the search scope choice; markup passes its value, the wiring uses `.selector`. */
export const SEARCH_SCOPE_ACTIONS = {
  setSearchScope: action('set-search-scope'),
} as const;

/** The two scopes a search can cover: the view it started from, or All Tickets. */
export type SearchScopeChoice = 'view' | 'all-tickets';

export interface SearchScopeToolbarProps {
  /** Whether the active search covers All Tickets rather than the view it started from. */
  allTickets: boolean;
  /** The view the search started from, named on the choice that returns to it. */
  viewLabel: string;
}

/**
 * The search scope switch (HS2-4J6764): a Kerf FloatingToolbar centered at the bottom of the search
 * results whose ToolbarControlGroup holds a two-choice SegmentedControl: the view the search started
 * from, or All Tickets (every collection except Trash). The owner renders it only while a search is
 * active outside Trash and positions it inside the ticket work area.
 */
export function SearchScopeToolbar({ allTickets, viewLabel }: SearchScopeToolbarProps) {
  return (
    <div class="search-scope-toolbar">
      <FloatingToolbar label="Search Scope" position="bottom">
        <ToolbarControlGroup>
          <SegmentedControl<SearchScopeChoice>
            id="search-scope"
            label="Search Scope"
            value={allTickets ? 'all-tickets' : 'view'}
            choices={[
              { value: 'view', label: viewLabel, title: `Search only ${viewLabel}` },
              { value: 'all-tickets', label: 'All Tickets', title: 'Search all tickets except Trash' },
            ]}
            action={SEARCH_SCOPE_ACTIONS.setSearchScope.value}
            appearance="toolbar"
            shape="pill"
            layout="content"
          />
        </ToolbarControlGroup>
      </FloatingToolbar>
    </div>
  );
}

/** Wire every SearchScopeToolbar under `root`; production rescopes the workspace search, the demo a fixture. */
export function wireSearchScopeToolbar(
  root: HTMLElement,
  bindings: { readonly setAllTickets: (allTickets: boolean) => void },
): () => void {
  return delegate(root, 'click', SEARCH_SCOPE_ACTIONS.setSearchScope.selector, (_event, target) => {
    const value = (target as HTMLElement).dataset.segmentValue;
    if (value === 'view' || value === 'all-tickets') bindings.setAllTickets(value === 'all-tickets');
  });
}
