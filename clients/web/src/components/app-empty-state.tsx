export function AppEmptyState() {
  return <section class="app-empty">
    <h1>Open a Hot Sheet project</h1>
    <p>Choose a code checkout to discover its ticket sources and start working.</p>
    <wa-button appearance="accent" data-action="add-project">Open project</wa-button>
  </section>;
}

export function ProjectRestoreState() {
  return <section class="app-empty" data-component="project-restore-state" role="status" aria-busy="true">
    <h1>Opening Hot Sheet</h1>
    <p>Restoring projects, tickets, and terminals…</p>
  </section>;
}
