import './ticket-reader.css';

import { TicketInspector, type TicketInspectorProps } from './ticket-inspector';

export type TicketReaderProps = Omit<TicketInspectorProps, 'presentation'> & {
  active?: boolean;
  projectName?: string;
  stackPosition?: number;
  stackSize?: number;
  readOnly?: boolean;
};

export function TicketReader({ active = true, projectName, stackPosition = 1, stackSize = 1, readOnly = false, ...props }: TicketReaderProps) {
  return <div class="ticket-reader" data-component="ticket-reader" data-large-text={String(props.largeText??false)} data-reader-active={String(active)} data-reader-position={stackPosition} data-reader-count={stackSize} role="dialog" aria-modal={active ? 'true' : undefined} aria-label={`${readOnly ? 'Read' : 'Read and edit'} ${props.slug}${projectName ? ` in ${projectName}` : ''}`}>
    {stackSize > 1 && <p class="ticket-reader__layer-context"><span>{projectName}</span><small>Reader {stackPosition} of {stackSize}</small></p>}
    <TicketInspector {...props} canUpdate={readOnly ? false : props.canUpdate} canAddNotes={readOnly ? false : props.canAddNotes} canEditNotes={readOnly ? false : props.canEditNotes} canDeleteNotes={readOnly ? false : props.canDeleteNotes} upNextEligible={readOnly ? false : props.upNextEligible} attachmentsEnabled={readOnly ? false : props.attachmentsEnabled} presentation="reader" />
  </div>;
}
