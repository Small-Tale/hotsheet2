/**
 * Which open project's server answers this computer's machine-wide settings: App Settings ›
 * Features and Standby (HS2-N3QBVG). They describe "this computer's Hot Sheet", so a local project
 * answers even while a remote tab is selected; a remote tab's server is another machine. Only when
 * no local project is open does the selected remote project answer, for want of any other.
 *
 * Remote tabs reach their server through this computer's bridge on the same origin as local ones,
 * so the re-read key names the source itself, not the URL origin: switching from a remote tab to
 * a local one must re-read, or the page keeps showing the other machine's choices.
 */
import { parseRemoteProjectId } from './remote-projects/identity';

export function machineSettingsProject<P extends { id: string }>(
  projects: readonly P[],
  selected: P | undefined,
): P | undefined {
  if (selected && !parseRemoteProjectId(selected.id)) return selected;
  return projects.find((item) => !parseRemoteProjectId(item.id)) ?? selected;
}

/** The re-read key of a settings source: this computer, or one remote binding's machine. */
export function machineSettingsKey(project: { id: string; apiPath: string }, origin: string): string {
  const binding = parseRemoteProjectId(project.id)?.bindingId;
  return binding ? `remote:${binding}` : `local:${new URL(project.apiPath, origin).origin}`;
}
