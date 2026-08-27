import { Paperclip } from 'lucide';
import { LucideIcon } from './lucide-icon';
import './ticket-inspector-panel.css';
export interface TicketAttachmentItem { id: string; name: string }
export const DEFAULT_ATTACHMENTS: readonly TicketAttachmentItem[] = [
  { id: 'wireframe', name: 'wireframe.png' },
  { id: 'requirements', name: 'requirements.md' },
];
export function TicketAttachments({ attachments = DEFAULT_ATTACHMENTS }: { attachments?: readonly TicketAttachmentItem[] }) {
  return <div class="ticket-inspector__content" data-component="ticket-attachments"><section><h2>Attachments</h2>{attachments.map(attachment => <div class="ticket-inspector__attachment" data-attachment-id={attachment.id}><LucideIcon icon={Paperclip} name="paperclip" /><span>{attachment.name}</span></div>)}<p>{attachments.length} {attachments.length === 1 ? 'attachment' : 'attachments'} total</p></section></div>;
}
