import '@awesome.me/webawesome/dist/components/button/button.js';
import '@awesome.me/webawesome/dist/components/dialog/dialog.js';
import '@awesome.me/webawesome/dist/components/tag/tag.js';
import './conversation-export-dialog.css';

import { FileJson2, FileText, FolderOpen, Image, Paperclip, RotateCw, Sparkles } from 'lucide';

import type { ConversationActivity, ConversationMessage } from '../ai-conversation';
import {
  conversationExportBundleEntries,
  type ConversationExportDraft,
  conversationExportMessageLabel,
  type ConversationExportSource,
  conversationExportValidation,
} from '../conversation-export';
import { LucideIcon } from './lucide-icon';
import { Select } from './select';

export interface ConversationExportDialogState {
  source: ConversationExportSource;
  messages: readonly ConversationMessage[];
  activity?: readonly ConversationActivity[];
  draft: ConversationExportDraft;
  busy?: boolean;
  error?: string;
  summaryAvailable?: boolean;
}

const bundleIcons = {
  manifest: FileJson2,
  transcript: FileText,
  structure: FileJson2,
  summary: Sparkles,
  attachments: Paperclip,
  media: Image,
};

export function ConversationExportDialog({ state }: { state?: ConversationExportDialogState }) {
  if (!state) return <></>;
  const { source, messages, draft, busy = false, error = '', summaryAvailable = true } = state;
  const rangeScope = draft.scope.kind === 'range' ? draft.scope : undefined;
  const range = Boolean(rangeScope);
  const startMessageId = rangeScope?.startMessageId ?? messages.at(0)?.id ?? '';
  const endMessageId = rangeScope?.endMessageId ?? messages.at(-1)?.id ?? '';
  const messageChoices = messages.map((message, index) => ({ value: message.id, label: conversationExportMessageLabel(message, index) }));
  const existing = draft.destination?.existing;
  const sameConversation = existing?.sourceConversationId === source.conversationId;
  const validation = conversationExportValidation(messages, draft, source);
  const summaryUnavailable = draft.bundle.includeSummary && !summaryAvailable;
  const bundleEntries = conversationExportBundleEntries(draft.bundle);
  const submitLabel = busy ? 'Saving…' : draft.writeMode === 'reexport' ? 'Re-export conversation' : draft.writeMode === 'overwrite' ? 'Overwrite export' : 'Save conversation';

  return <wa-dialog class="conversation-export-dialog" data-component="conversation-export-dialog" label="Save conversation" open with-footer>
    <form class="conversation-export-dialog__form" data-action="submit-conversation-export">
      <p class="conversation-export-dialog__intro">Save a portable conversation bundle that Hot Sheet can reopen later.</p>

      <fieldset class="conversation-export-dialog__section">
        <legend>Messages</legend>
        <label class="conversation-export-dialog__choice">
          <input type="radio" name="conversation-export-scope" value="all" checked={!range} disabled={busy} />
          <span><strong>Entire conversation</strong><small>All {messages.length} messages, in their original order.</small></span>
        </label>
        <label class="conversation-export-dialog__choice">
          <input type="radio" name="conversation-export-scope" value="range" checked={range} disabled={busy} />
          <span><strong>Selected range</strong><small>Include both boundary messages and everything between them.</small></span>
        </label>
        <div class="conversation-export-dialog__range" aria-label="Selected message range" aria-disabled={String(!range)}>
          <Select name="conversation-export-start-message" label="From" value={startMessageId} choices={messageChoices} disabled={!range || busy} />
          <Select name="conversation-export-end-message" label="Through" value={endMessageId} choices={messageChoices} disabled={!range || busy} />
        </div>
      </fieldset>

      <fieldset class="conversation-export-dialog__section">
        <legend>Destination</legend>
        <div class="conversation-export-dialog__destination" data-selected={String(Boolean(draft.destination))}>
          <span class="conversation-export-dialog__destination-icon"><LucideIcon icon={FolderOpen} name="folder-open" /></span>
          <span><strong>{draft.destination ? 'Conversation bundle' : 'No destination selected'}</strong><small title={draft.destination?.displayPath}>{draft.destination?.displayPath ?? 'Choose a parent folder for the .hotsheet-chat bundle.'}</small></span>
          <wa-button type="button" appearance="outlined" size="small" data-action="pick-conversation-export-destination" disabled={busy}>{draft.destination ? 'Change' : 'Choose…'}</wa-button>
        </div>
        {existing && <div class="conversation-export-dialog__collision" role="status">
          <wa-tag variant="warning">Existing export</wa-tag>
          <p>{sameConversation ? `Revision ${existing.revision} of this conversation is already there.` : 'This destination contains a different conversation export.'}</p>
          {sameConversation && <label class="conversation-export-dialog__choice">
            <input type="radio" name="conversation-export-write-mode" value="reexport" checked={draft.writeMode === 'reexport'} disabled={busy} />
            <span><strong>Re-export as the next revision</strong><small>Keep lineage to revision {existing.revision} in the manifest.</small></span>
          </label>}
          <label class="conversation-export-dialog__choice conversation-export-dialog__choice--danger">
            <input type="radio" name="conversation-export-write-mode" value="overwrite" checked={draft.writeMode === 'overwrite'} disabled={busy} />
            <span><strong>Overwrite the existing bundle</strong><small>Replace its files in this destination.</small></span>
          </label>
        </div>}
      </fieldset>

      <fieldset class="conversation-export-dialog__section">
        <legend>Bundle contents</legend>
        <div class="conversation-export-dialog__options">
          <label class="conversation-export-dialog__choice"><input type="checkbox" name="conversation-export-attachments" checked={draft.bundle.includeAttachments} disabled={busy} /><span><strong>Attachments</strong><small>Copy non-media files referenced by the selected messages.</small></span></label>
          <label class="conversation-export-dialog__choice"><input type="checkbox" name="conversation-export-media" checked={draft.bundle.includeMedia} disabled={busy} /><span><strong>Original media</strong><small>Copy referenced images, audio, and video at their original quality.</small></span></label>
          <label class="conversation-export-dialog__choice"><input type="checkbox" name="conversation-export-summary" checked={draft.bundle.includeSummary} disabled={busy || !summaryAvailable} /><span><strong>Concise summary</strong><small>{summaryAvailable ? 'Add a local summary.md without replacing the complete transcript.' : 'Summary export is unavailable for this conversation.'}</small></span></label>
        </div>
        <div class="conversation-export-dialog__bundle" aria-label="Files in export bundle">
          {bundleEntries.map(entry => <span data-bundle-purpose={entry.purpose}><LucideIcon icon={bundleIcons[entry.purpose]} name={entry.purpose} />{entry.path}</span>)}
        </div>
        <p class="conversation-export-dialog__resume"><LucideIcon icon={RotateCw} name="rotate-cw" /><span><strong>Reopen metadata is always included.</strong> {source.sessionId && source.resumable !== false ? 'A tail-ending export can resume the original session; an earlier range reopens as a read-only transcript.' : 'This conversation has no resumable session ID, so it reopens as a read-only transcript.'}</span></p>
      </fieldset>

      <p class="conversation-export-dialog__error" role="alert">{error || (summaryUnavailable ? 'Summary export is unavailable for this conversation.' : validation)}</p>
      <div slot="footer" class="conversation-export-dialog__actions">
        <wa-button type="button" appearance="outlined" data-action="cancel-conversation-export" disabled={busy}>Cancel</wa-button>
        <wa-button type="submit" appearance="accent" disabled={busy || Boolean(validation) || summaryUnavailable}>{submitLabel}</wa-button>
      </div>
    </form>
  </wa-dialog>;
}
