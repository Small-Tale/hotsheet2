import { signal } from 'kerfjs';

/** OS files can stage evidence on an open project tab without intercepting ticket drags. */
export function acceptsProjectTabFileDrag(
  draggingTickets: boolean,
  destinationExists: boolean,
  transfer: Pick<DataTransfer, 'types'> | null,
): boolean {
  return !draggingTickets && destinationExists && Array.from(transfer?.types ?? []).includes('Files');
}

/** A drop needs actual files; some external drags advertise Files without delivering any. */
export function projectTabDroppedFiles(
  draggingTickets: boolean,
  destinationExists: boolean,
  transfer: Pick<DataTransfer, 'files'> | null,
): File[] {
  return !draggingTickets && destinationExists ? Array.from(transfer?.files ?? []) : [];
}

/** What a drag over a project tab would do there: copy dragged tickets, or stage OS files. */
export type ProjectTabDropKind = 'ticket' | 'file';
export interface ProjectTabDropTarget {
  projectId: string;
  kind: ProjectTabDropKind;
}

/**
 * The project tab a drag is over (HS2-6KNWYT). The tab renders its drop-target state from this
 * signal through Kerf's `dropTarget` prop; an attribute set on the tab's DOM node was dropped by
 * the next shell render's morph while the drag was still over the tab.
 */
export const projectTabDropTarget = signal<ProjectTabDropTarget | undefined>(undefined);

/** Mark `projectId` as the drag's target, writing the signal only when the target changes. */
export function markProjectTabDropTarget(projectId: string, kind: ProjectTabDropKind): void {
  const current = projectTabDropTarget.value;
  if (current?.projectId === projectId && current.kind === kind) return;
  projectTabDropTarget.value = { projectId, kind };
}

/** Clear the drop target, or only `projectId`'s (of `kind`, when given) so a stale leave cannot clear a newer tab. */
export function clearProjectTabDropTarget(projectId?: string, kind?: ProjectTabDropKind): void {
  const current = projectTabDropTarget.value;
  if (!current) return;
  if (projectId !== undefined && current.projectId !== projectId) return;
  if (kind !== undefined && current.kind !== kind) return;
  projectTabDropTarget.value = undefined;
}

/** A project tab's drop-target kind, for its `dropTarget` prop. */
export function projectTabDropKind(projectId: string): ProjectTabDropKind | undefined {
  const current = projectTabDropTarget.value;
  return current?.projectId === projectId ? current.kind : undefined;
}
