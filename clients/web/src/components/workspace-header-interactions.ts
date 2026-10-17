import { delegate } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import {
  nextWorkspaceSort,
  type WorkspaceSort,
  type WorkspaceSortDirection,
  type WorkspaceViewMode,
} from './workspace-controls';

/**
 * Side effects for the WorkspaceHeader sort menu and its narrow-width overflow menu (HS2-AAX2B5).
 * Production passes the workspace preferences and ticket operations; the UX demo passes fixture
 * callbacks, so both resolve the same sort cycling and overflow actions.
 */
export interface WorkspaceHeaderActionBindings {
  /** The current sort, read when a sort choice arrives so a repeated choice flips direction. */
  readonly currentSort: () => { sort: WorkspaceSort; direction: WorkspaceSortDirection };
  readonly sort: (next: { sort: WorkspaceSort; direction: WorkspaceSortDirection }) => void;
  readonly toggleSelectedUpNext: () => void;
  /** Open the selected tickets' actions menu anchored below `anchor`. */
  readonly openSelectedTicketActions: (anchor: HTMLElement) => void;
  readonly openSearch: () => void;
  readonly setViewMode: (mode: WorkspaceViewMode) => void;
}

/** Wire the sort menu and overflow menu of every WorkspaceHeader or WorkspaceControls under `root`. */
export function wireWorkspaceHeaderActions(root: HTMLElement, bindings: WorkspaceHeaderActionBindings): () => void {
  const lifetime = createScope();
  const sortBy = (choice: WorkspaceSort) => {
    const current = bindings.currentSort();
    bindings.sort(nextWorkspaceSort(current.sort, current.direction, choice));
  };
  lifetime.add(
    // A click, not `change`, so choosing the current sort again still reverses its direction.
    delegate(root, 'click', 'kui-select[name="workspace-sort"] .kui-select__option', (_event, target) => {
      sortBy((target as HTMLElement).dataset.value as WorkspaceSort);
    }),
  );
  lifetime.add(
    delegate(root, 'kui-menu-select', '[data-workspace-overflow]', (event) => {
      const item = (event as CustomEvent<{ item: HTMLElement }>).detail.item,
        { workspaceOverflowAction: action, viewMode, workspaceSort } = item.dataset;
      if (action === 'toggle-selected-up-next') bindings.toggleSelectedUpNext();
      else if (action === 'open-selected-ticket-actions') bindings.openSelectedTicketActions(item);
      else if (action === 'open-workspace-search') bindings.openSearch();
      else if (action === 'set-view-mode' && viewMode) bindings.setViewMode(viewMode as WorkspaceViewMode);
      else if (action === 'set-workspace-sort' && workspaceSort) sortBy(workspaceSort as WorkspaceSort);
    }),
  );
  return () => {
    lifetime.dispose();
  };
}
