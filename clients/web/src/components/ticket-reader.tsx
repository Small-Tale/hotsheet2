import './ticket-reader.css';

import { remify } from '@kerfjs/ui/css-values';
import { List } from '@kerfjs/ui/list';
import { Row } from '@kerfjs/ui/row';
import { DialogSurface } from '@kerfjs/ui/surface-scaffold';
import { Text } from '@kerfjs/ui/text';
import type { SafeHtml } from 'kerfjs';

import { INSPECTOR_AND_EDITOR_TARGETS } from '../interaction-attrs/inspector-and-editor';
import { TicketInspector, type TicketInspectorProps } from './ticket-inspector';

export type TicketReaderProps = Omit<TicketInspectorProps, 'presentation'> & {
  active?: boolean;
  frameId?: string;
  open?: boolean;
  projectName?: string;
  stackPosition?: number;
  stackSize?: number;
  readOnly?: boolean;
  /** Popover/menu surfaces that must live inside this modal dialog to stay interactive (HS2-EZ10RS). */
  overlay?: SafeHtml;
};

export type TicketReaderDialogElement = HTMLDialogElement;

export function TicketReader({
  active = true,
  frameId = 'ticket-reader',
  open = true,
  projectName,
  stackPosition = 1,
  stackSize = 1,
  readOnly = false,
  overlay,
  ...props
}: TicketReaderProps) {
  const label = `${readOnly ? 'Read' : 'Read and edit'} ${props.slug}${projectName ? ` in ${projectName}` : ''}`;
  return (
    <DialogSurface
      className="ticket-reader-dialog"
      label={label}
      withoutHeader
      preferredWidth={remify(1024)}
      height="viewport"
      bodyInset="none"
      bodyLayout="fill"
      stack={{
        open,
        active: active && open,
        position: stackPosition,
        count: stackSize,
        offset: active ? undefined : remify(-10 * (stackSize - stackPosition)),
      }}
      rootAttributes={{
        ...INSPECTOR_AND_EDITOR_TARGETS.ticketReaderDialog.attrs,
        'data-key': frameId,
        'data-reader-frame-id': frameId,
        'data-reader-active': String(active),
        'data-reader-position': String(stackPosition),
        'data-reader-count': String(stackSize),
        'data-large-text': String(props.largeText ?? false),
      }}
    >
      {open && (
        <div
          class="ticket-reader"
          {...INSPECTOR_AND_EDITOR_TARGETS.ticketReader.attrs}
          data-reader-frame-id={frameId}
          data-reader-active={String(active)}
          data-large-text={String(props.largeText ?? false)}
        >
          {stackSize > 1 && (
            <List dividerSides="b" textInsets="rl" controlInsets="tb">
              <Row hAlign="full" vAlign="middle" gap="m">
                <Text variant="span" size="compact" wrap="truncate">
                  <strong>{projectName}</strong>
                </Text>
                <Text variant="span" size="compact" tone="quiet" wrap="nowrap">
                  Reader {stackPosition} of {stackSize}
                </Text>
              </Row>
            </List>
          )}
          <TicketInspector
            {...props}
            canUpdate={readOnly ? false : props.canUpdate}
            canAddNotes={readOnly ? false : props.canAddNotes}
            canEditNotes={readOnly ? false : props.canEditNotes}
            canDeleteNotes={readOnly ? false : props.canDeleteNotes}
            upNextEligible={readOnly ? false : props.upNextEligible}
            attachmentsEnabled={readOnly ? false : props.attachmentsEnabled}
            presentation="reader"
          />
          {overlay}
        </div>
      )}
    </DialogSurface>
  );
}
