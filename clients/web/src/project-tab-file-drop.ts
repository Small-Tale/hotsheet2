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
