import '@awesome.me/webawesome/dist/components/button/button.js';
import '@awesome.me/webawesome/dist/components/dialog/dialog.js';
import '@awesome.me/webawesome/dist/components/tag/tag.js';
import './conversation-export-dialog.css';

import { FileJson2, FileText, FolderOpen, Image, Paperclip, Sparkles } from 'lucide';

import type { ConversationActivity, ConversationMessage } from '../ai-conversation';
import {
  conversationExportBundleEntries,
  type ConversationExportDraft,
  type ConversationExportSource,
  conversationExportValidation,
  selectedConversationMessages,
} from '../conversation-export';
import { LucideIcon } from './lucide-icon';

export interface ConversationExportDialogState {
  source: ConversationExportSource;
  messages: readonly ConversationMessage[];
  activity?: readonly ConversationActivity[];
  draft: ConversationExportDraft;
  busy?: boolean;
  error?: string;
  summaryAvailable?: boolean;
  step?: 1|2|3;
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
  const { source, messages, draft, busy = false, error = '', summaryAvailable = true, step = 1 } = state;
  const range = draft.scope.kind === 'range';
  const selectedMessages=selectedConversationMessages(messages,draft.scope);
  const selectedIds=new Set(selectedMessages.map(message=>message.id));
  const existing = draft.destination?.existing;
  const sameConversation = existing?.sourceConversationId === source.conversationId;
  const validation = conversationExportValidation(messages, draft, source);
  const blockingValidation=validation==='Choose where to save the conversation.'?undefined:validation;
  const summaryUnavailable = draft.bundle.includeSummary && !summaryAvailable;
  const bundleEntries = conversationExportBundleEntries(draft.bundle);
  const submitLabel = busy ? 'Saving…' : draft.writeMode === 'reexport' ? 'Re-export conversation' : draft.writeMode === 'overwrite' ? 'Overwrite export' : 'Save conversation';

  return <wa-dialog class="conversation-export-dialog" data-component="conversation-export-dialog" data-step={step} label="Save conversation" open with-footer>
    <form class="conversation-export-dialog__form" data-action="submit-conversation-export">
      <header class="conversation-export-dialog__header"><span>Step {step} of 3</span><strong>{step===1?'Choose messages':step===2?'Choose contents':'Review and save'}</strong></header>

      {step===1&&<fieldset class="conversation-export-dialog__section">
        <legend>Messages</legend>
        <label class="conversation-export-dialog__choice">
          <input type="radio" name="conversation-export-scope" value="all" checked={!range} disabled={busy} />
          <span><strong>Entire conversation</strong><small>All {messages.length} messages, in their original order.</small></span>
        </label>
        <label class="conversation-export-dialog__choice">
          <input type="radio" name="conversation-export-scope" value="range" checked={range} disabled={busy} />
          <span><strong>Selected range</strong><small>Include both boundary messages and everything between them.</small></span>
        </label>
        <p class="conversation-export-dialog__range-help">{range?'Pick one message to start a new range, then another to include everything between them.':'Switch to Selected range, then pick messages directly from the transcript.'}</p>
        <div class="conversation-export-dialog__messages" aria-label="Conversation messages">
          {messages.map((message,index)=><button type="button" data-action="pick-conversation-export-message" data-message-id={message.id} data-selected={String(!range||selectedIds.has(message.id))} disabled={busy||!range}><strong>{index+1}. {message.role==='user'?'You':'Assistant'}</strong><span>{message.content.trim()||'Empty message'}</span></button>)}
        </div>
        <p class="conversation-export-dialog__selection-summary" role="status">{selectedMessages.length} of {messages.length} messages selected</p>
      </fieldset>}

      {step===2&&<fieldset class="conversation-export-dialog__section">
        <legend>Bundle contents</legend>
        <div class="conversation-export-dialog__options">
          <label class="conversation-export-dialog__choice"><input type="checkbox" name="conversation-export-attachments" checked={draft.bundle.includeAttachments} disabled={busy} /><span><strong>Attachments</strong><small>Copy non-media files referenced by the selected messages.</small></span></label>
          <label class="conversation-export-dialog__choice"><input type="checkbox" name="conversation-export-media" checked={draft.bundle.includeMedia} disabled={busy} /><span><strong>Original media</strong><small>Copy referenced images, audio, and video at their original quality.</small></span></label>
          <label class="conversation-export-dialog__choice"><input type="checkbox" name="conversation-export-summary" checked={draft.bundle.includeSummary} disabled={busy || !summaryAvailable} /><span><strong>Concise summary</strong><small>{summaryAvailable ? 'Add a local summary.md without replacing the complete transcript.' : 'Summary export is unavailable for this conversation.'}</small></span></label>
        </div>
        <div class="conversation-export-dialog__bundle" aria-label="Files in export bundle">
          {bundleEntries.map(entry => <span data-bundle-purpose={entry.purpose}><LucideIcon icon={bundleIcons[entry.purpose]} name={entry.purpose} />{entry.path}</span>)}
        </div>
      </fieldset>}

      {step===3&&<section class="conversation-export-dialog__review" aria-label="Export review">
        <div><strong>{selectedMessages.length} message{selectedMessages.length===1?'':'s'}</strong><span>{range?'Selected transcript range':'Entire conversation'}</span></div>
        <div><strong>{bundleEntries.length} bundle item{bundleEntries.length===1?'':'s'}</strong><span>{draft.bundle.includeSummary?'Includes concise summary':'Complete transcript and structure'}</span></div>
        <p>Saved conversations can be reopened in Hot Sheet. A partial range opens read-only; a tail-ending export can continue its original session.</p>
        <div class="conversation-export-dialog__destination" data-selected={String(Boolean(draft.destination))}>
          <span class="conversation-export-dialog__destination-icon"><LucideIcon icon={FolderOpen} name="folder-open" /></span>
          <span><strong>{draft.destination?'Save location selected':'Choose location when you save'}</strong><small title={draft.destination?.displayPath}>{draft.destination?.displayPath??'The final Save action opens the system folder picker.'}</small></span>
        </div>
        {existing&&<div class="conversation-export-dialog__collision" role="status">
          <wa-tag variant="warning">Existing export</wa-tag>
          <p>{sameConversation?`Revision ${existing.revision} of this conversation is already there.`:'This destination contains a different conversation export.'}</p>
          {sameConversation&&<label class="conversation-export-dialog__choice"><input type="radio" name="conversation-export-write-mode" value="reexport" checked={draft.writeMode==='reexport'} disabled={busy}/><span><strong>Re-export as the next revision</strong><small>Keep lineage to revision {existing.revision} in the manifest.</small></span></label>}
          <label class="conversation-export-dialog__choice conversation-export-dialog__choice--danger"><input type="radio" name="conversation-export-write-mode" value="overwrite" checked={draft.writeMode==='overwrite'} disabled={busy}/><span><strong>Overwrite the existing bundle</strong><small>Replace its files in this destination.</small></span></label>
        </div>}
      </section>}

      <p class="conversation-export-dialog__error" role="alert">{error || (summaryUnavailable ? 'Summary export is unavailable for this conversation.' : blockingValidation)}</p>
      <div slot="footer" class="conversation-export-dialog__actions">
        <button type="button" class="conversation-export-dialog__button" data-action="cancel-conversation-export" disabled={busy}>Cancel</button>
        {step>1&&<button type="button" class="conversation-export-dialog__button" data-action="previous-conversation-export-step" disabled={busy}>Back</button>}
        <button type="button" class="conversation-export-dialog__button conversation-export-dialog__button--accent" data-action={step<3?'next-conversation-export-step':'finish-conversation-export'} disabled={busy||(step<3?selectedMessages.length===0:Boolean(blockingValidation)||summaryUnavailable)}>{step<3?'Continue':draft.destination?submitLabel:'Choose destination and save'}</button>
      </div>
    </form>
  </wa-dialog>;
}
