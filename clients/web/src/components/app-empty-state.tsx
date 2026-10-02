import './app-empty-state.css';

import { PROJECT_LIFECYCLE_ACTIONS } from '../interaction-attrs/project-lifecycle';

export function AppEmptyState() {
  return (
    <section class="app-empty">
      <h1>Open a Hot Sheet project</h1>
      <p>Choose a code checkout to discover its ticket sources and start working.</p>
      <wa-button appearance="accent" {...PROJECT_LIFECYCLE_ACTIONS.addProject.attrs}>
        Open project
      </wa-button>
    </section>
  );
}

export function ProjectRestoreState() {
  return (
    <section class="app-empty" data-component="project-restore-state" role="status" aria-busy="true">
      <h1>Opening Hot Sheet</h1>
      <p>Restoring projects, tickets, and terminals…</p>
    </section>
  );
}

export interface AppMessageStateProps {
  title: string;
  message: string;
}

/** A centered, action-free full-surface message in the empty-state presentation. */
export function AppMessageState({ title, message }: AppMessageStateProps) {
  return (
    <section class="app-empty">
      <h1>{title}</h1>
      <p>{message}</p>
    </section>
  );
}
