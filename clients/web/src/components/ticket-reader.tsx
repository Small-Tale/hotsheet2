import './ticket-reader.css';

import { TicketInspector, type TicketInspectorProps } from './ticket-inspector';

export type TicketReaderProps = Omit<TicketInspectorProps, 'presentation'>;

export function TicketReader(props: TicketReaderProps) {
  return <div class="ticket-reader" data-component="ticket-reader" role="dialog" aria-modal="true" aria-label={`Read and edit ${props.slug}`}>
    <TicketInspector {...props} presentation="reader" />
  </div>;
}
