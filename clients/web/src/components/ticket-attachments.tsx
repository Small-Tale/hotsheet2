import './ticket-inspector-panel.css';

import { Paperclip, Plus, Upload } from 'lucide';

import { LucideIcon } from './lucide-icon';

export interface TicketAttachmentItem { id: string; name: string }
export const DEFAULT_ATTACHMENTS: readonly TicketAttachmentItem[] = [
  { id: 'wireframe', name: 'wireframe.png' },
  { id: 'requirements', name: 'requirements.md' },
];
export function TicketAttachments({ attachments = DEFAULT_ATTACHMENTS }: { attachments?: readonly TicketAttachmentItem[] }) {
  return <div class="ticket-inspector__content ticket-attachments" data-component="ticket-attachments" data-attachment-drop-target="true"><section><header class="ticket-inspector__section-header"><h2>Attachments</h2><label class="ticket-attachments__browse"><LucideIcon icon={Plus} name="plus" /><span>Add</span><input type="file" name="ticket-attachments" multiple aria-label="Browse and add attachments" /></label></header>{attachments.map(attachment => <div class="ticket-inspector__attachment" data-attachment-id={attachment.id}><LucideIcon icon={Paperclip} name="paperclip" /><span>{attachment.name}</span></div>)}<label class="ticket-attachments__drop"><LucideIcon icon={Upload} name="upload" /><span>Drop attachments here or browse</span><input type="file" name="ticket-attachments" multiple aria-label="Drop or browse attachments" /></label><p>{attachments.length} {attachments.length === 1 ? 'attachment' : 'attachments'} total</p></section></div>;
}
