import './ticket-inspector-panel.css';
export interface TicketTimelineEntry { id: string; time: string; text: string }
export const DEFAULT_TIMELINE_ENTRIES: readonly TicketTimelineEntry[] = [
  { id: 'active', time: 'Now', text: 'Development is active on this ticket.' },
  { id: 'metadata', time: '1 hour ago', text: 'Ticket metadata was updated.' },
];
export function TicketTimeline({ entries = DEFAULT_TIMELINE_ENTRIES }: { entries?: readonly TicketTimelineEntry[] }) {
  return <div class="ticket-inspector__content" data-component="ticket-timeline"><section><h2>Timeline</h2><ol class="ticket-inspector__timeline">{entries.map(entry => <li data-entry-id={entry.id}><time>{entry.time}</time><p>{entry.text}</p></li>)}</ol><p>{entries.length} {entries.length === 1 ? 'note' : 'notes'} total</p></section></div>;
}
