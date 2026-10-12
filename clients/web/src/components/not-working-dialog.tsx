import '@awesome.me/webawesome/dist/components/button/button.js';
import '@awesome.me/webawesome/dist/components/dialog/dialog.js';
import './not-working-dialog.css';

import { List } from '@kerfjs/ui/list';
import { Row } from '@kerfjs/ui/row';
import { FieldLabel, Text } from '@kerfjs/ui/text';

import { TICKET_SELECTION_ACTIONS, TICKET_SELECTION_TARGETS } from '../interaction-attrs/ticket-selection';
import { type PendingAttachment, PendingAttachmentPicker } from './pending-attachment-picker';

export interface NotWorkingDialogProps {
  slug: string;
  mode?: 'not-working' | 'reopen';
  open?: boolean;
  note: string;
  attachments: readonly PendingAttachment[];
  notesEnabled?: boolean;
  attachmentsEnabled?: boolean;
  submitting?: boolean;
  error?: string;
}

export function NotWorkingDialog({
  slug,
  mode = 'not-working',
  open,
  note,
  attachments,
  notesEnabled = true,
  attachmentsEnabled = true,
  submitting = false,
  error = '',
}: NotWorkingDialogProps) {
  const empty = (!notesEnabled || note.trim().length === 0) && attachments.length === 0;
  const reopening = mode === 'reopen',
    title = reopening ? `Reopen Ticket — ${slug}` : `Not Working — ${slug}`,
    prompt = reopening ? 'What needs another attempt?' : 'What’s wrong?',
    submitLabel = reopening ? 'Reopen Ticket' : 'Report Not Working';
  return (
    <wa-dialog
      class="not-working-dialog"
      {...TICKET_SELECTION_TARGETS.notWorkingDialog.attrs}
      role="dialog"
      label={title}
      aria-label={title}
      open={open}
      data-controlled-open={String(Boolean(open))}
    >
      <form {...TICKET_SELECTION_ACTIONS.submitNotWorking.attrs}>
        <List gap="m">
          {notesEnabled ? (
            <div class="not-working-dialog__note">
              <FieldLabel id="not-working-note-label">{prompt}</FieldLabel>
              <textarea
                name="not-working-note"
                aria-labelledby="not-working-note-label"
                rows={5}
                disabled={submitting}
                placeholder="Describe what failed or what needs another attempt…"
                autofocus
              >
                {note}
              </textarea>
            </div>
          ) : (
            <Text flush size="compact" tone="quiet">
              This ticket provider does not support notes. Add an attachment to report the problem.
            </Text>
          )}
          <PendingAttachmentPicker attachments={attachments} enabled={attachmentsEnabled && !submitting} />
          {!attachmentsEnabled && (
            <Text flush size="compact" tone="quiet">
              This ticket provider does not support attachments.
            </Text>
          )}
          <div class="not-working-dialog__error" role="alert">
            {error && (
              <Text flush size="compact" tone="danger">
                {error}
              </Text>
            )}
          </div>
          <Row hAlign="right" vAlign="middle" gap="xs">
            <wa-button
              type="button"
              appearance="plain"
              {...TICKET_SELECTION_ACTIONS.cancelNotWorking.attrs}
              disabled={submitting}
            >
              Cancel
            </wa-button>
            <wa-button type="submit" variant="danger" disabled={submitting || empty}>
              {submitting ? 'Submitting…' : submitLabel}
            </wa-button>
          </Row>
        </List>
      </form>
    </wa-dialog>
  );
}
