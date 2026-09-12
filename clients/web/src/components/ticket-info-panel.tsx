import '@awesome.me/webawesome/dist/components/button/button.js';
import './ticket-inspector-panel.css';

import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { CircleAlert, Plus } from 'lucide';

import type {AttachmentReferenceContext} from '../attachment-references';
import {DETAILS_FEEDBACK_ID,textRequestsFeedback} from '../feedback-needed';
import type { InlineFeedbackReply } from '../feedback-replies';
import { MarkdownEditor, type MarkdownEditorMode } from './markdown-editor';
import { MenuHeader } from './menu-header';
import { MenuItem } from './menu-item';
import {FeedbackPrompt,type NoteCardProps} from './note-card';
import type { TicketStatus } from './status-badge';
import { BlockedBadge } from './status-badge';
import { TicketCategorySelect } from './ticket-category-select';
import { TicketNotes } from './ticket-notes';
import { TicketPrioritySelect } from './ticket-priority-select';
import type { TicketPriority } from './ticket-row';
import { TicketStatusMenu } from './ticket-status-menu';
import { TicketTagEditor } from './ticket-tag-editor';

export interface TicketInfoPanelProps { status: TicketStatus; priority: TicketPriority; category: string; tags: string[]; tagSuggestions?: readonly string[]; tagPopoverId?: string; canUpdate?: boolean; canEditText?: boolean; canAddNotes?: boolean; canEditNotes?: boolean; canDeleteNotes?: boolean; composingNote?: boolean; composerDraft?: string; details: string; detailsMode?: MarkdownEditorMode; detailsDirty?: boolean; readerPresentation?: boolean; feedbackNeeded?:boolean; notes?: readonly NoteCardProps[]; editingNoteId?: string; noteDraft?: string; inlineFeedbackReplies?: Readonly<Record<string, readonly InlineFeedbackReply[]>>; feedbackChoiceSelections?: Readonly<Record<string, readonly string[]>>; blockedReason?: string; blockedReasonEditing?: boolean; blockedReasonDraft?: string; providerName?: string; updatedLabel?: string;attachmentContext?:AttachmentReferenceContext }
export function TicketInfoPanel({ status, priority, category, tags, tagSuggestions, tagPopoverId, canUpdate = true, canEditText = canUpdate, canAddNotes = true, canEditNotes = true, canDeleteNotes = true, composingNote = false, composerDraft = '', details, detailsMode = 'preview', detailsDirty = false, readerPresentation = false, feedbackNeeded=false, notes = [], editingNoteId, noteDraft, inlineFeedbackReplies, feedbackChoiceSelections, blockedReason = '', blockedReasonEditing = false, blockedReasonDraft = blockedReason, providerName = 'Hot Sheet git', updatedLabel = 'Updated now',attachmentContext }: TicketInfoPanelProps) {
  const detailsFeedback=feedbackNeeded&&textRequestsFeedback(details)&&!notes.some(note=>note.kind==='feedback_needed');
  const resolvedTagPopoverId=tagPopoverId??'ticket-tag-popover';
  return <div class="ticket-inspector__content" data-component="ticket-info-panel">
    <section class="ticket-inspector__metadata" aria-label="Ticket metadata">
      <TicketCategorySelect name="inspector-category" value={category} disabled={!canUpdate} />
      <TicketPrioritySelect name="inspector-priority" value={priority} disabled={!canUpdate} />
      <div><span>Status</span><span class="ticket-inspector__status-line"><TicketStatusMenu value={status} disabled={!canUpdate} />{blockedReason && <BlockedBadge />}</span></div>
    </section>
    <section class="ticket-inspector__section ticket-inspector__blocked-section">{blockedReasonEditing ? <><MenuHeader label="Blocked reason"/><div class="ticket-inspector__blocked-editor"><textarea name="blocked-reason" aria-label="Blocked reason">{blockedReasonDraft}</textarea></div></> : blockedReason ? <><MenuHeader label="Blocked reason"/><div class="ticket-inspector__blocked-surface" data-edit-blocked-reason={canEditText?'true':undefined} role={canEditText?'button':undefined} tabIndex={canEditText?0:undefined} aria-label={canEditText?'Edit blocked reason':undefined} title={canEditText?'Double-click to edit':undefined}><p>{blockedReason}</p></div></> : canEditText ? <MenuItem className="ticket-inspector__block-action" action="edit-blocked-reason" icon={<LucideIcon icon={Plus} name="plus"/>} label="Block ticket"/> : undefined}</section>
    <section class="ticket-inspector__section ticket-inspector__details-section"><MenuHeader label="Details"/><div class="ticket-inspector__details-surface" data-feedback-needed={detailsFeedback?'true':undefined}>{detailsFeedback&&readerPresentation&&canAddNotes?<div class="ticket-inspector__details-feedback" data-details-feedback="true"><header class="ticket-inspector__details-feedback-header"><LucideIcon icon={CircleAlert} name="circle-alert" />Feedback needed</header><FeedbackPrompt source={details} id={DETAILS_FEEDBACK_ID} inlineReplies={inlineFeedbackReplies?.[DETAILS_FEEDBACK_ID]} selectedChoices={feedbackChoiceSelections?.[DETAILS_FEEDBACK_ID]} attachmentContext={attachmentContext}/><div class="note-card__editor"><textarea name="note-body" data-note-id={DETAILS_FEEDBACK_ID} data-note-response="true" aria-label="Feedback response" placeholder="Additional response (optional)">{noteDraft??''}</textarea><div><wa-button size="small" appearance="outlined" data-action="dismiss-feedback" data-note-id={DETAILS_FEEDBACK_ID} title="Clear this feedback request without replying">No response needed</wa-button><wa-button size="small" appearance="accent" data-action="save-note-edit" data-note-id={DETAILS_FEEDBACK_ID} data-note-response="true">Respond</wa-button></div></div></div>:<><MarkdownEditor value={details} mode={detailsMode} dirty={detailsDirty} appearance="embedded" showExpand={false} label="Ticket details" editable={canEditText} attachmentContext={attachmentContext} />{detailsFeedback&&canAddNotes&&<wa-button class="note-card__respond" appearance="outlined" data-action="respond-to-feedback" data-note-id={DETAILS_FEEDBACK_ID}>Respond to Feedback</wa-button>}</>}</div></section>
    <section class="ticket-inspector__section"><MenuHeader label="Tags" action={canUpdate?'open-ticket-tag-popover':undefined} actionLabel="Add tag" actionIcon={canUpdate?Plus:undefined} actionIconName={canUpdate?'plus':undefined} actionPopoverTarget={canUpdate?resolvedTagPopoverId:undefined}/><TicketTagEditor tags={tags} suggestions={tagSuggestions} editable={canUpdate} popoverId={resolvedTagPopoverId} /></section>
    <TicketNotes notes={notes} editingNoteId={editingNoteId} noteDraft={noteDraft} composing={composingNote} composerDraft={composerDraft} canAdd={canAddNotes} canEdit={canEditNotes} canDelete={canDeleteNotes} readerMode={readerPresentation} inlineFeedbackReplies={inlineFeedbackReplies} feedbackChoiceSelections={feedbackChoiceSelections} attachmentContext={attachmentContext} />
    <footer class="ticket-inspector__provenance"><span>{providerName}</span><span>{updatedLabel}</span></footer>
  </div>;
}
