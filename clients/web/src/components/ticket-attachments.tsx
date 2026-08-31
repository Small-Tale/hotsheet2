import './ticket-inspector-panel.css';

import { Clipboard, Download, ExternalLink, Paperclip, Plus, Trash2, Upload } from 'lucide';

import { LucideIcon } from './lucide-icon';

export interface TicketAttachmentItem { id: string; name: string }
export const DEFAULT_ATTACHMENTS: readonly TicketAttachmentItem[] = [
  { id: 'wireframe', name: 'wireframe.png' },
  { id: 'requirements', name: 'requirements.md' },
];
export function TicketAttachments({ attachments = DEFAULT_ATTACHMENTS, enabled = true, message = '' }: { attachments?: readonly TicketAttachmentItem[]; enabled?: boolean; message?: string }) {
  return <div class="ticket-inspector__content ticket-attachments" data-component="ticket-attachments" data-attachment-drop-target={String(enabled)}><section><header class="ticket-inspector__section-header"><h2>Attachments</h2>{enabled&&<label class="ticket-attachments__browse"><LucideIcon icon={Plus} name="plus" /><span>Add</span><input type="file" name="ticket-attachments" multiple aria-label="Browse and add attachments" /></label>}</header>{attachments.map(attachment => <div class="ticket-inspector__attachment" data-attachment-id={attachment.id}><LucideIcon icon={Paperclip} name="paperclip" /><span>{attachment.name}</span>{enabled&&<span class="ticket-inspector__attachment-actions"><button type="button" data-action="open-attachment" data-attachment-action-id={attachment.id} aria-label={`Open ${attachment.name}`}><LucideIcon icon={ExternalLink} name="external-link" /></button><button type="button" data-action="download-attachment" data-attachment-action-id={attachment.id} data-attachment-name={attachment.name} aria-label={`Download ${attachment.name}`}><LucideIcon icon={Download} name="download" /></button><button type="button" data-action="copy-attachment-reference" data-attachment-action-id={attachment.id} aria-label={`Copy reference to ${attachment.name}`}><LucideIcon icon={Clipboard} name="clipboard" /></button><button type="button" data-action="remove-attachment" data-attachment-action-id={attachment.id} aria-label={`Remove ${attachment.name}`}><LucideIcon icon={Trash2} name="trash-2" /></button></span>}</div>)}{enabled?<label class="ticket-attachments__drop"><LucideIcon icon={Upload} name="upload" /><span>Drop attachments here or browse</span><input type="file" name="ticket-attachments" multiple aria-label="Drop or browse attachments" /></label>:<p class="ticket-attachments__unsupported">This provider does not support attachment actions.</p>}{message&&<p class="ticket-attachments__status" role="status">{message}</p>}<p>{attachments.length} {attachments.length === 1 ? 'attachment' : 'attachments'} total</p></section></div>;
}
