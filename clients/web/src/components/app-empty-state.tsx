import { EmptyState } from '@kerfjs/ui/empty-state';
import { List } from '@kerfjs/ui/list';

import { PROJECT_LIFECYCLE_ACTIONS } from '../interaction-attrs/project-lifecycle';

export function AppEmptyState() {
  return (
    <List fill hAlign="center" vAlign="middle">
      <EmptyState
        title="Open a Hot Sheet project"
        detail="Choose a code checkout to discover its ticket sources and start working."
        action={
          <wa-button appearance="accent" {...PROJECT_LIFECYCLE_ACTIONS.addProject.attrs}>
            Open project
          </wa-button>
        }
      />
    </List>
  );
}

export function ProjectRestoreState() {
  return (
    <List fill hAlign="center" vAlign="middle" rootAttributes={{ 'data-project-restore-state': 'true' }}>
      <EmptyState title="Opening Hot Sheet" detail="Restoring projects, tickets, and terminals…" busy />
    </List>
  );
}

export interface AppMessageStateProps {
  title: string;
  message: string;
}

/** A centered, action-free full-surface message in the empty-state presentation. */
export function AppMessageState({ title, message }: AppMessageStateProps) {
  return (
    <List fill hAlign="center" vAlign="middle">
      <EmptyState title={title} detail={message} />
    </List>
  );
}
