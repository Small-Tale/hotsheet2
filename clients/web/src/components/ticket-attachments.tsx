import { Paperclip } from 'lucide';
import { LucideIcon } from './lucide-icon';
import './ticket-inspector-panel.css';
export function TicketAttachments({ attachmentCount = 2 }: { attachmentCount?: number }) {
  return <div class="ticket-inspector__content" data-component="ticket-attachments"><section><h2>Attachments</h2><div class="ticket-inspector__attachment"><LucideIcon icon={Paperclip} name="paperclip" /><span>wireframe.png</span></div><p>{attachmentCount} attachments total</p></section></div>;
}
