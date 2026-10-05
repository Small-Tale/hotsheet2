import './pending-attachment-picker.css';

import { uiColor } from '@kerfjs/ui/css-values';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Paperclip, Plus, Trash2, Upload } from 'lucide';

import { TICKET_SELECTION_ACTIONS, TICKET_SELECTION_TARGETS } from '../interaction-attrs/ticket-selection';

export interface PendingAttachment {
  id: string;
  name: string;
}

export function PendingAttachmentPicker({
  attachments,
  enabled = true,
}: {
  attachments: readonly PendingAttachment[];
  enabled?: boolean;
}) {
  if (!enabled) return null;
  return (
    <section
      class="pending-attachment-picker"
      data-component="pending-attachment-picker"
      {...TICKET_SELECTION_TARGETS.notWorkingDropzone.attrs}
    >
      <header>
        <h3>Attachments</h3>
        <label>
          <LucideIcon size={14.4} icon={Plus} name="plus" />
          <span>Add</span>
          <input type="file" multiple name="not-working-attachments" aria-label="Browse evidence attachments" />
        </label>
      </header>
      {attachments.length > 0 && (
        <div class="pending-attachment-picker__items">
          {attachments.map((item) => (
            <div class="pending-attachment-picker__item" data-pending-attachment-id={item.id}>
              <span class="pending-attachment-picker__item-icon">
                <LucideIcon icon={Paperclip} name="paperclip" size={14.4} color={uiColor('neutral-on-quiet')} />
              </span>
              <span class="pending-attachment-picker__item-name" title={item.name}>
                {item.name}
              </span>
              <button
                type="button"
                {...TICKET_SELECTION_ACTIONS.removeNotWorkingAttachment.attrs}
                data-pending-attachment-id={item.id}
                aria-label={`Remove ${item.name}`}
              >
                <LucideIcon size={14.4} icon={Trash2} name="trash-2" />
              </button>
            </div>
          ))}
        </div>
      )}
      <label class="pending-attachment-picker__drop">
        <LucideIcon size={14.4} icon={Upload} name="upload" />
        <span>Drop attachments here or browse</span>
        <input
          type="file"
          multiple
          name="not-working-attachments"
          aria-label="Add evidence by dropping or choosing files"
        />
      </label>
    </section>
  );
}
