import './ticket-inspector-panel.css';

import { MoreHorizontal, Paperclip, Plus, Upload } from 'lucide';

import {isGalleryMediaAttachment,isVideoAttachment} from '../attachment-references';
import { LucideIcon } from './lucide-icon';

export interface TicketAttachmentItem { id: string; name: string;url?:string }
export const DEFAULT_ATTACHMENTS: readonly TicketAttachmentItem[] = [
  { id: 'wireframe', name: 'wireframe.png' },
  { id: 'requirements', name: 'requirements.md' },
];
export function TicketAttachments({ attachments = DEFAULT_ATTACHMENTS, enabled = true, message = '' }: { attachments?: readonly TicketAttachmentItem[]; enabled?: boolean; message?: string }) {
  const media=attachments.filter((attachment):attachment is TicketAttachmentItem&{url:string}=>Boolean(attachment.url&&isGalleryMediaAttachment(attachment.name)));
  return <div class="ticket-inspector__content ticket-attachments" data-component="ticket-attachments" data-attachment-drop-target={String(enabled)}><section><header class="ticket-inspector__section-header"><h2>Attachments</h2>{enabled&&<label class="ticket-attachments__browse"><LucideIcon icon={Plus} name="plus" /><span>Add</span><input type="file" name="ticket-attachments" multiple aria-label="Browse and add attachments" /></label>}</header>{attachments.map(attachment => <div class="ticket-inspector__attachment" data-component="ticket-attachment-item" data-action={enabled ? 'open-attachment-row' : undefined} data-attachment-id={attachment.id} data-attachment-action-id={enabled ? attachment.id : undefined} data-attachment-name={enabled ? attachment.name : undefined} data-attachment-url={enabled ? attachment.url : undefined} data-attachment-menu-kind={enabled ? 'item' : undefined}><LucideIcon icon={Paperclip} name="paperclip" /><span title={`${attachment.name} — double-click to open`}>{attachment.name}</span>{enabled&&<button class="ticket-inspector__attachment-menu" type="button" data-action="open-attachment-menu" aria-label={`More actions for ${attachment.name}`} title={`More actions for ${attachment.name}`}><LucideIcon icon={MoreHorizontal} name="more-horizontal" /></button>}</div>)}{enabled?<label class="ticket-attachments__drop"><LucideIcon icon={Upload} name="upload" /><span>Drop attachments here or browse</span><input type="file" name="ticket-attachments" multiple aria-label="Drop or browse attachments" /></label>:<p class="ticket-attachments__unsupported">This provider does not support attachment actions.</p>}{message&&<p class="ticket-attachments__status" role="status">{message}</p>}<p>{attachments.length} {attachments.length === 1 ? 'attachment' : 'attachments'} total</p>{media.length>0&&<div class="ticket-attachments__image-grid" aria-label="Attached media">{media.map(item=><button type="button" data-action="open-attachment-gallery" data-gallery-attachment-id={item.id} data-attachment-url={item.url} data-attachment-name={item.name} aria-label={`Open ${item.name} in media gallery`}>{isVideoAttachment(item.name)?<video src={item.url} aria-label={item.name} preload="metadata" muted playsInline/>:<img src={item.url} alt={item.name}/>}</button>)}</div>}</section></div>;
}
