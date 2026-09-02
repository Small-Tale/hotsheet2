import '@awesome.me/webawesome/dist/components/button/button.js';
import '@awesome.me/webawesome/dist/components/input/input.js';
import '@awesome.me/webawesome/dist/components/option/option.js';
import '@awesome.me/webawesome/dist/components/select/select.js';
import './quick-ticket-composer.css';

import { Paperclip, Plus, Trash2, Upload } from 'lucide';

import { LucideIcon } from './lucide-icon';
import { TicketCategorySelect } from './ticket-category-select';

export interface QuickTicketComposerProps {
  expanded?: boolean;
  title?: string;
  category?: string;
  providerName?: string;
  canCreate?: boolean;
  attachments?: readonly { id: string; name: string }[];
  attachmentsEnabled?: boolean;
  attachmentMessage?: string;
  attachmentError?: boolean;
  busy?: boolean;
  submitting?: boolean;
}

export function focusQuickTicketComposerTitle(root: ParentNode): boolean {
  const input = root.querySelector<HTMLElement>('[name="new-ticket-title"]');
  if (!input) return false;
  input.focus({ preventScroll: true });
  return true;
}

export function QuickTicketComposer({ expanded = false, title = '', category = 'task', providerName = 'Hot Sheet', canCreate = true, attachments = [], attachmentsEnabled = true, attachmentMessage = '', attachmentError = false, busy = false, submitting = false }: QuickTicketComposerProps) {
  if (!expanded) return <button type="button" class="quick-ticket-composer__launcher" data-component="quick-ticket-composer" data-action="expand-ticket-composer" data-new-ticket-drop-target="true" title={attachmentsEnabled ? 'Create a new ticket or drop attachment files here' : 'Create a new ticket'}><LucideIcon icon={Plus} name="plus" />New ticket…</button>;
  return <form class="quick-ticket-composer" data-component="quick-ticket-composer" data-action="create-ticket-form" data-new-ticket-drop-target="true" data-submitting={String(submitting)}>
    <wa-input name="new-ticket-title" label="Ticket title" value={title} autofocus required></wa-input>
    <TicketCategorySelect name="new-ticket-category" label="Category" value={category} />
    <section class="quick-ticket-composer__attachments" aria-label="New ticket attachments">
      <header><span><LucideIcon icon={Paperclip} name="paperclip" />Attachments</span>{attachmentsEnabled && <label><LucideIcon icon={Plus} name="plus" />Add<input type="file" multiple name="new-ticket-attachments" aria-label="Browse attachments for new ticket" /></label>}</header>
      {attachments.length > 0 && <div class="quick-ticket-composer__attachment-list">{attachments.map(item => <div class="quick-ticket-composer__attachment" data-pending-attachment-id={item.id}><LucideIcon icon={Paperclip} name="paperclip" /><span title={item.name}>{item.name}</span><button type="button" data-action="remove-new-ticket-attachment" data-pending-attachment-id={item.id} aria-label={`Remove ${item.name}`} title={`Remove ${item.name}`}><LucideIcon icon={Trash2} name="trash-2" /></button></div>)}</div>}
      {attachmentsEnabled ? <label class="quick-ticket-composer__drop"><LucideIcon icon={Upload} name="upload" /><span>Drop attachment files anywhere in this area or browse</span><input type="file" multiple name="new-ticket-attachments" aria-label="Drop or browse attachments for new ticket" /></label> : <p class="quick-ticket-composer__notice">This ticket provider does not support attachments.</p>}
      {attachmentMessage && <p class={attachmentError ? 'quick-ticket-composer__message quick-ticket-composer__message--error' : 'quick-ticket-composer__message'} role={attachmentError ? 'alert' : 'status'}>{attachmentMessage}</p>}
    </section>
    <div class="quick-ticket-composer__footer">
      <span>Creating in {providerName}</span>
      <div><wa-button type="button" appearance="plain" data-action="cancel-ticket-composer" disabled={submitting}>Cancel</wa-button><wa-button type="submit" appearance="accent" disabled={!canCreate || busy || submitting}>{submitting ? 'Creating…' : 'Create ticket'}</wa-button></div>
    </div>
    {!canCreate && <p class="quick-ticket-composer__notice" role="status">This ticket provider does not support creating tickets.</p>}
  </form>;
}
