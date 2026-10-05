import type { TerminalDashboardGroup } from './components/terminal-dashboard';

/** Per-project request generations let independent resyncs settle without discarding each other. */
export class TerminalSnapshotRefresh {
  private versions = new Map<string, number>();

  begin(projectId: string): number {
    const version = (this.versions.get(projectId) ?? 0) + 1;
    this.versions.set(projectId, version);
    return version;
  }

  merge(
    previous: readonly TerminalDashboardGroup[],
    results: readonly { projectId: string; version: number; group?: TerminalDashboardGroup }[],
    openProjects: readonly string[],
  ): TerminalDashboardGroup[] {
    const groups = new Map(previous.map((group) => [group.projectId, group]));
    for (const result of results) {
      // A failed snapshot isn't a halt resolution; only an authoritative successful result replaces it.
      if (this.versions.get(result.projectId) === result.version && result.group)
        groups.set(result.projectId, result.group);
    }
    return openProjects.flatMap((id) => {
      const group = groups.get(id);
      return group ? [group] : [];
    });
  }
}
