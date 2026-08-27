import './ticket-inspector-panel.css';
export function TicketTimeline({ noteCount = 3 }: { noteCount?: number }) {
  return <div class="ticket-inspector__content" data-component="ticket-timeline"><section><h2>Timeline</h2><ol class="ticket-inspector__timeline"><li><time>Now</time><p>Development is active on this ticket.</p></li><li><time>1 hour ago</time><p>Ticket metadata was updated.</p></li></ol><p>{noteCount} notes total</p></section></div>;
}
