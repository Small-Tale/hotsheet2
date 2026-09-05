import '@awesome.me/webawesome/dist/components/button/button.js';
import './ticket-inspector-panel.css';

import { Plus } from 'lucide';

import type {AttachmentReferenceContext} from '../attachment-references';
import {DETAILS_FEEDBACK_ID,textRequestsFeedback} from '../feedback-needed';
import type { InlineFeedbackReply } from '../feedback-replies';
import { LucideIcon } from './lucide-icon';
import { MarkdownEditor, type MarkdownEditorMode } from './markdown-editor';
import {FeedbackPrompt,type NoteCardProps} from './note-card';
import type { TicketStatus } from './status-badge';
import { BlockedBadge } from './status-badge';
import { TicketCategorySelect } from './ticket-category-select';
import { TicketNotes } from './ticket-notes';
import { TicketPrioritySelect } from './ticket-priority-select';
import type { TicketPriority } from './ticket-row';
import { TicketStatusMenu } from './ticket-status-menu';
import { TicketTagEditor } from './ticket-tag-editor';

export interface TicketInfoPanelProps { status: TicketStatus; priority: TicketPriority; category: string; tags: string[]; tagSuggestions?: readonly string[]; canUpdate?: boolean; canAddNotes?: boolean; canEditNotes?: boolean; canDeleteNotes?: boolean; composingNote?: boolean; composerDraft?: string; details: string; detailsMode?: MarkdownEditorMode; detailsDirty?: boolean; readerPresentation?: boolean; feedbackNeeded?:boolean; notes?: readonly NoteCardProps[]; editingNoteId?: string; noteDraft?: string; inlineFeedbackReplies?: Readonly<Record<string, readonly InlineFeedbackReply[]>>; feedbackChoiceSelections?: Readonly<Record<string, readonly string[]>>; blockedReason?: string; blockedReasonEditing?: boolean; blockedReasonDraft?: string; providerName?: string; updatedLabel?: string;attachmentContext?:AttachmentReferenceContext }
export function TicketInfoPanel({ status, priority, category, tags, tagSuggestions, canUpdate = true, canAddNotes = true, canEditNotes = true, canDeleteNotes = true, composingNote = false, composerDraft = '', details, detailsMode = 'preview', detailsDirty = false, readerPresentation = false, feedbackNeeded=false, notes = [], editingNoteId, noteDraft, inlineFeedbackReplies, feedbackChoiceSelections, blockedReason = '', blockedReasonEditing = false, blockedReasonDraft = blockedReason, providerName = 'Hot Sheet git', updatedLabel = 'Updated now',attachmentContext }: TicketInfoPanelProps) {
  const detailsFeedback=feedbackNeeded&&textRequestsFeedback(details)&&!notes.some(note=>note.kind==='feedback_needed');
  return <div class="ticket-inspector__content" data-component="ticket-info-panel">
    <section class="ticket-inspector__metadata" aria-label="Ticket metadata">
      <TicketCategorySelect name="inspector-category" value={category} />
      <TicketPrioritySelect name="inspector-priority" value={priority} />
      <div><span>Status</span><span class="ticket-inspector__status-line"><TicketStatusMenu value={status} />{blockedReason && <BlockedBadge />}</span></div>
    </section>
    <section class="ticket-inspector__section ticket-inspector__blocked-section">{blockedReasonEditing ? <><header class="ticket-inspector__section-header"><h2>Blocked reason</h2></header><div class="ticket-inspector__blocked-editor"><textarea name="blocked-reason" aria-label="Blocked reason">{blockedReasonDraft}</textarea></div></> : blockedReason ? <><header class="ticket-inspector__section-header"><h2>Blocked reason</h2></header><div class="ticket-inspector__blocked-surface" data-edit-blocked-reason={canUpdate?'true':undefined} role={canUpdate?'button':undefined} tabIndex={canUpdate?0:undefined} aria-label={canUpdate?'Edit blocked reason':undefined} title={canUpdate?'Double-click to edit':undefined}><p>{blockedReason}</p></div></> : canUpdate ? <button type="button" class="ticket-inspector__empty-action ticket-inspector__block-action" data-action="edit-blocked-reason"><LucideIcon icon={Plus} name="plus" />Block ticket</button> : undefined}</section>
    <section class="ticket-inspector__section ticket-inspector__details-section"><header class="ticket-inspector__section-header"><h2>Details</h2></header><div class="ticket-inspector__details-surface">{detailsFeedback&&readerPresentation&&canAddNotes?<div class="ticket-inspector__details-feedback" data-details-feedback="true"><FeedbackPrompt source={details} id={DETAILS_FEEDBACK_ID} inlineReplies={inlineFeedbackReplies?.[DETAILS_FEEDBACK_ID]} selectedChoices={feedbackChoiceSelections?.[DETAILS_FEEDBACK_ID]} attachmentContext={attachmentContext}/><div class="note-card__editor"><textarea name="note-body" data-note-id={DETAILS_FEEDBACK_ID} data-note-response="true" aria-label="Feedback response" placeholder="Additional response (optional)">{noteDraft??''}</textarea><div><wa-button size="small" appearance="outlined" data-action="dismiss-feedback" data-note-id={DETAILS_FEEDBACK_ID} title="Clear this feedback request without replying">No response needed</wa-button><wa-button size="small" appearance="accent" data-action="save-note-edit" data-note-id={DETAILS_FEEDBACK_ID} data-note-response="true">Respond</wa-button></div></div></div>:<><MarkdownEditor value={details} mode={detailsMode} dirty={detailsDirty} appearance="embedded" showExpand={false} label="Ticket details" editable={canUpdate} />{detailsFeedback&&canAddNotes&&<wa-button class="note-card__respond" appearance="outlined" data-action="respond-to-feedback" data-note-id={DETAILS_FEEDBACK_ID}>Respond to Feedback</wa-button>}</>}</div></section>
    <section class="ticket-inspector__section"><header class="ticket-inspector__section-header"><h2>Tags</h2></header><TicketTagEditor tags={tags} suggestions={tagSuggestions} editable={canUpdate} /></section>
    <TicketNotes notes={notes} editingNoteId={editingNoteId} noteDraft={noteDraft} composing={composingNote} composerDraft={composerDraft} canAdd={canAddNotes} canEdit={canEditNotes} canDelete={canDeleteNotes} readerMode={readerPresentation} inlineFeedbackReplies={inlineFeedbackReplies} feedbackChoiceSelections={feedbackChoiceSelections} attachmentContext={attachmentContext} />
    <footer class="ticket-inspector__provenance"><span>{providerName}</span><span>{updatedLabel}</span></footer>
  </div>;
}
