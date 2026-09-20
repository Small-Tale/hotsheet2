import type { CorruptTicket } from '../api';
import { CorruptTicketInspector, type CorruptTicketRecoveryState } from './corrupt-ticket-row';
import { TicketInspector, type TicketInspectorProps } from './ticket-inspector';
import { TicketInspectorPlaceholder } from './ticket-inspector-placeholder';

export function Inspector(props: TicketInspectorProps) {
  return <TicketInspector {...props} />;
}

export function InspectorPlaceholder({ selectionCount }: { selectionCount: number }) {
  return <TicketInspectorPlaceholder selectionCount={selectionCount} />;
}

export function CorruptInspector({ ticket, recovery, selectionCount }: {
  ticket?: CorruptTicket;
  recovery?: CorruptTicketRecoveryState;
  selectionCount: number;
}) {
  return ticket
    ? <CorruptTicketInspector ticket={ticket} recovery={recovery} />
    : <InspectorPlaceholder selectionCount={selectionCount} />;
}
