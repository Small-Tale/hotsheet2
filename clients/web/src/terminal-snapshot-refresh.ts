import type { TerminalDashboardGroup } from './components/terminal-dashboard';

/** A quiet resync should not publish unchanged session data into the app-wide render signal. */
export function sameTerminalDashboardSnapshot(
  previous: readonly TerminalDashboardGroup[],
  next: readonly TerminalDashboardGroup[],
): boolean {
  return sameSnapshotValue(previous, next);
}

/** Compare the JSON-shaped snapshot without copying potentially large terminal scrollback. */
function sameSnapshotValue(previous: unknown, next: unknown): boolean {
  if (previous === next) return true;
  if (previous === null || next === null || typeof previous !== 'object' || typeof next !== 'object') return false;
  if (Array.isArray(previous) !== Array.isArray(next)) return false;
  const previousValues = previous as Record<string, unknown>;
  const nextValues = next as Record<string, unknown>;
  const keys = Object.keys(previousValues);
  if (keys.length !== Object.keys(nextValues).length) return false;
  return keys.every((key) => Object.hasOwn(nextValues, key) && sameSnapshotValue(previousValues[key], nextValues[key]));
}

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
