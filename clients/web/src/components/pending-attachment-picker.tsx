import './pending-attachment-picker.css';

import { Button } from '@kerfjs/ui/button';
import { flex, px, uiColor } from '@kerfjs/ui/css-values';
import { FileDropZone, FilePickerTrigger } from '@kerfjs/ui/file-picker';
import { List } from '@kerfjs/ui/list';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Row } from '@kerfjs/ui/row';
import { SunkenPanel } from '@kerfjs/ui/sunken-panel';
import { Text } from '@kerfjs/ui/text';
import { Paperclip, Plus, Trash2, Upload } from 'lucide';

import { TICKET_SELECTION_ACTIONS, TICKET_SELECTION_TARGETS } from '../interaction-attrs/ticket-selection';

/** `wireFilePickers` id shared by the picker's Add trigger and drop zone. */
export const PENDING_ATTACHMENT_PICKER = 'pending-attachments';

export interface PendingAttachment {
  id: string;
  name: string;
}

/**
 * Evidence files chosen before submission (HS2-0NGK56): the header, rows, and remove controls compose
 * Kerf List/Row/Text/SunkenPanel and a plain Web Awesome button; the Add trigger and drop target are Kerf
 * FilePickerTrigger and FileDropZone (HS2-RB7KXZ), reported through `wireFilePickers`.
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
          <FilePickerTrigger
            label="Add"
            accessibleLabel="Browse evidence attachments"
            icon={<LucideIcon icon={Plus} name="plus" />}
            multiple
            name="not-working-attachments"
            pickerId={PENDING_ATTACHMENT_PICKER}
          />
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
                  <Button
                    emphasis="plain"
                    size="compact"
                    action={TICKET_SELECTION_ACTIONS.removeNotWorkingAttachment.value}
                    rootAttributes={{ 'data-pending-attachment-id': item.id }}
                  >
                    {/* The labeled icon names Web Awesome's inner button (a host aria-label does not). */}
                    <LucideIcon size={14.4} icon={Trash2} name="trash-2" label={`Remove ${item.name}`} />
                  </Button>
                </Row>
              </SunkenPanel>
            ))}
          </List>
        ) : (
          <></>
        )}
        <FileDropZone
          title="Drop attachments here or browse"
          accessibleLabel="Add evidence by dropping or choosing files"
          icon={<LucideIcon icon={Upload} name="upload" />}
          multiple
          name="not-working-attachments"
          pickerId={PENDING_ATTACHMENT_PICKER}
        />
      </List>
    </section>
  );
}
