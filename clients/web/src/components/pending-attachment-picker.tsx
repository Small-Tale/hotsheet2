import './pending-attachment-picker.css';
import '@awesome.me/webawesome/dist/components/button/button.js';

import { flex, px, uiColor } from '@kerfjs/ui/css-values';
import { List } from '@kerfjs/ui/list';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Row } from '@kerfjs/ui/row';
import { SunkenPanel } from '@kerfjs/ui/sunken-panel';
import { Text } from '@kerfjs/ui/text';
import { Paperclip, Plus, Trash2, Upload } from 'lucide';

import { TICKET_SELECTION_ACTIONS, TICKET_SELECTION_TARGETS } from '../interaction-attrs/ticket-selection';

export interface PendingAttachment {
  id: string;
  name: string;
}

/**
 * Evidence files chosen before submission (HS2-0NGK56): the header, rows, and remove controls compose
 * Kerf List/Row/Text/SunkenPanel and a plain Web Awesome button; only the file-picker labels and the drop target
 * keep app CSS, since Kerf has no file-drop or file-picker trigger component yet (KF-RDV6V9).
 */
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
      <List gap="xs">
        <Row hAlign="space-between" vAlign="middle">
          <Text variant="h3" size="compact" flush>
            Attachments
          </Text>
          <label class="pending-attachment-picker__add">
            <LucideIcon size={14.4} icon={Plus} name="plus" />
            <span>Add</span>
            <input type="file" multiple name="not-working-attachments" aria-label="Browse evidence attachments" />
          </label>
        </Row>
        {attachments.length > 0 ? (
          <List gap="2xs">
            {attachments.map((item) => (
              <SunkenPanel>
                <Row vAlign="middle" gap="xs" rootAttributes={{ 'data-pending-attachment-id': item.id }}>
                  <LucideIcon icon={Paperclip} name="paperclip" size={14.4} color={uiColor('neutral-on-quiet')} />
                  <Row flex={flex(1, 1, px(0))}>
                    <Text variant="span" size="compact" wrap="truncate" title={item.name}>
                      {item.name}
                    </Text>
                  </Row>
                  <wa-button
                    appearance="plain"
                    size="small"
                    {...TICKET_SELECTION_ACTIONS.removeNotWorkingAttachment.attrs}
                    data-pending-attachment-id={item.id}
                  >
                    {/* The labeled icon names Web Awesome's inner button (a host aria-label does not). */}
                    <LucideIcon size={14.4} icon={Trash2} name="trash-2" label={`Remove ${item.name}`} />
                  </wa-button>
                </Row>
              </SunkenPanel>
            ))}
          </List>
        ) : (
          <></>
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
      </List>
    </section>
  );
}
