// The header family's stylesheet, shared with WorkspaceControls (which loads it first): the header renders its root block.
import './workspace-header.css';

import { Toolbar } from '@kerfjs/ui/toolbar';

import {
  defaultWorkspaceSortDirection,
  WorkspaceControls,
  type WorkspaceHeaderProps,
  WorkspaceIdentity,
} from './workspace-controls';

export * from './workspace-controls';

/** Project identity beside the workspace controls in one real Toolbar (demo and standalone use). */
export function WorkspaceHeader({
  projectName,
  mode,
  presentation = 'toolbar',
  searchOpen = false,
  searchModel,
  searchHelpOpen = false,
  sort = 'updated',
  sortDirection = defaultWorkspaceSortDirection(sort),
  controlsVisible = true,
  notificationCount = 0,
  selectedTicketCount = 0,
  selectedTicketsUpNext = 'none',
  selectedTicketsUpNextEligible = false,
  selectedTicketsMutable = true,
}: WorkspaceHeaderProps) {
  return (
    <Toolbar
      className="workspace-header"
      dividerSides=""
      responsive="none"
      leading={
        <WorkspaceIdentity
          projectName={projectName}
          searchOpen={searchOpen && mode !== 'notifications' && mode !== 'settings'}
        />
      }
      trailing={
        controlsVisible ? (
          <WorkspaceControls
            mode={mode}
            presentation={presentation}
            searchOpen={searchOpen}
            searchModel={searchModel}
            searchHelpOpen={searchHelpOpen}
            sort={sort}
            sortDirection={sortDirection}
            notificationCount={notificationCount}
            selectedTicketCount={selectedTicketCount}
            selectedTicketsUpNext={selectedTicketsUpNext}
            selectedTicketsUpNextEligible={selectedTicketsUpNextEligible}
            selectedTicketsMutable={selectedTicketsMutable}
          />
        ) : undefined
      }
    />
  );
}
