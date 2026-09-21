import '@awesome.me/webawesome/dist/components/dialog/dialog.js';
import './ticket-reader.css';

import type { SafeHtml } from 'kerfjs';

import { TicketInspector, type TicketInspectorProps } from './ticket-inspector';

export type TicketReaderProps = Omit<TicketInspectorProps, 'presentation'> & {
  active?: boolean;
  frameId?: string;
  open?: boolean;
  projectName?: string;
  stackPosition?: number;
  stackSize?: number;
  readOnly?: boolean;
  style?: string;
  /** Popover/menu surfaces that must live inside this modal dialog to stay interactive (HS2-EZ10RS). */
  overlay?: SafeHtml;
};

export interface TicketReaderDialogElement extends HTMLElement {
  open: boolean;
  show(): Promise<void>;
}

export function showTicketReaderDialog(root: ParentNode, frameId: string): boolean {
  const dialog = root.querySelector<TicketReaderDialogElement>(
    `[data-component="ticket-reader"][data-reader-frame-id="${CSS.escape(frameId)}"]`,
  );
  if (!dialog) return false;
  const nativeDialog = dialog.shadowRoot?.querySelector('dialog');
  if (nativeDialog) nativeDialog.setAttribute('role', 'presentation');
  if (dialog.open && (!nativeDialog || nativeDialog.open)) return false;
  void dialog.show();
  return true;
}

export function TicketReader({
  active = true,
  frameId = 'ticket-reader',
  open = true,
  projectName,
  stackPosition = 1,
  stackSize = 1,
  readOnly = false,
  style,
  overlay,
  ...props
}: TicketReaderProps) {
  const label = `${readOnly ? 'Read' : 'Read and edit'} ${props.slug}${projectName ? ` in ${projectName}` : ''}`;
  return (
    <wa-dialog
      class="ticket-reader-dialog"
      data-component="ticket-reader"
      data-key={frameId}
      data-reader-frame-id={frameId}
      data-large-text={String(props.largeText ?? false)}
      data-reader-active={String(active)}
      data-reader-position={stackPosition}
      data-reader-count={stackSize}
      label={label}
      role="dialog"
      aria-modal={active ? 'true' : undefined}
      aria-label={label}
      without-header
      open={open || undefined}
      aria-hidden={open ? undefined : 'true'}
      inert={open ? undefined : true}
      style={style}
    >
      {open && (
        <div class="ticket-reader">
          {stackSize > 1 && (
            <p class="ticket-reader__layer-context">
              <span>{projectName}</span>
              <small>
                Reader {stackPosition} of {stackSize}
              </small>
            </p>
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
    </wa-dialog>
  );
}
