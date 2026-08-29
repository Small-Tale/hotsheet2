import './ticket-inspector-panel.css';
export interface TicketTimelineEntry { id: string; time: string; title: string; subtitle?: string; emphasized?: boolean }
export const DEFAULT_TIMELINE_ENTRIES: readonly TicketTimelineEntry[] = [
  { id: 'started', time: '1h ago', title: 'Claude started work', subtitle: 'Plan: build the shared inspector and timeline states.', emphasized: true },
  { id: 'metadata', time: '42m ago', title: 'Updated ticket metadata', subtitle: 'Set category, priority, and status.' },
  { id: 'implementation', time: '18m ago', title: 'Implemented inspector interactions', subtitle: 'Added blocked-state editing and reader navigation.', emphasized: true },
  { id: 'review', time: 'Now', title: 'Ready for review', subtitle: 'All checks pass. Please review the changes.', emphasized: true },
];
export function TicketTimeline({ entries = DEFAULT_TIMELINE_ENTRIES }: { entries?: readonly TicketTimelineEntry[] }) {
  return <div class="ticket-inspector__content" data-component="ticket-timeline"><section><h2>Timeline</h2><ol class="ticket-inspector__timeline">{entries.map(entry => <li data-entry-id={entry.id} data-emphasized={String(Boolean(entry.emphasized))}><time>{entry.time}</time><div><strong>{entry.title}</strong>{entry.subtitle && <p>{entry.subtitle}</p>}</div></li>)}</ol><p>{entries.length} {entries.length === 1 ? 'event' : 'events'} total</p></section></div>;
}
