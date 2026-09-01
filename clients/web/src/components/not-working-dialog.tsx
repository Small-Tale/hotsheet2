import '@awesome.me/webawesome/dist/components/dialog/dialog.js';
import './not-working-dialog.css';

import {
  type PendingAttachment,
  PendingAttachmentPicker,
} from './pending-attachment-picker';

export interface NotWorkingDialogProps {
  slug: string;
  open?: boolean;
  note: string;
  attachments: readonly PendingAttachment[];
  notesEnabled?: boolean;
  attachmentsEnabled?: boolean;
  submitting?: boolean;
  error?: string;
}

export function NotWorkingDialog({ slug, open = true, note, attachments, notesEnabled = true, attachmentsEnabled = true, submitting = false, error = '' }: NotWorkingDialogProps) {
  const empty = (!notesEnabled || note.trim().length === 0) && attachments.length === 0;
  return <wa-dialog class="not-working-dialog" data-component="not-working-dialog" role="dialog" label={`Not Working — ${slug}`} aria-label={`Not Working — ${slug}`} open={open}>
    <form data-action="submit-not-working" class="not-working-dialog__form">
      {notesEnabled ? <label class="not-working-dialog__note"><span>What’s wrong?</span><textarea name="not-working-note" rows={5} disabled={submitting} placeholder="Describe what failed or what needs another attempt…" autofocus>{note}</textarea></label> : <p class="not-working-dialog__hint">This ticket provider does not support notes. Add an attachment to report the problem.</p>}
      <PendingAttachmentPicker attachments={attachments} enabled={attachmentsEnabled && !submitting} />
      {!attachmentsEnabled && <p class="not-working-dialog__hint">This ticket provider does not support attachments.</p>}
      <p class="not-working-dialog__error" role="alert">{error}</p>
      <footer><button type="button" data-action="cancel-not-working" disabled={submitting}>Cancel</button><button type="submit" class="not-working-dialog__submit" disabled={submitting || empty}>{submitting ? 'Submitting…' : 'Report Not Working'}</button></footer>
    </form>
  </wa-dialog>;
}
