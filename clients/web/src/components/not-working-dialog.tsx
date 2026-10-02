import '@awesome.me/webawesome/dist/components/dialog/dialog.js';
import './not-working-dialog.css';

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
      <form {...TICKET_SELECTION_ACTIONS.submitNotWorking.attrs} class="not-working-dialog__form">
        {notesEnabled ? (
          <label class="not-working-dialog__note">
            <span>{prompt}</span>
            <textarea
              name="not-working-note"
              rows={5}
              disabled={submitting}
              placeholder="Describe what failed or what needs another attempt…"
              autofocus
            >
              {note}
            </textarea>
          </label>
        ) : (
          <p class="not-working-dialog__hint">
            This ticket provider does not support notes. Add an attachment to report the problem.
          </p>
        )}
        <PendingAttachmentPicker attachments={attachments} enabled={attachmentsEnabled && !submitting} />
        {!attachmentsEnabled && (
          <p class="not-working-dialog__hint">This ticket provider does not support attachments.</p>
        )}
        <p class="not-working-dialog__error" role="alert">
          {error}
        </p>
        <footer>
          <button type="button" {...TICKET_SELECTION_ACTIONS.cancelNotWorking.attrs} disabled={submitting}>
            Cancel
          </button>
          <button type="submit" class="not-working-dialog__submit" disabled={submitting || empty}>
            {submitting ? 'Submitting…' : submitLabel}
          </button>
        </footer>
      </form>
    </wa-dialog>
  );
}
