import '@awesome.me/webawesome/dist/components/button/button.js';
import './ticket-inspector-panel.css';

import { MarkdownEditor, type MarkdownEditorMode } from './markdown-editor';
import type { NoteCardProps } from './note-card';
import type { TicketStatus } from './status-badge';
import { BlockedBadge } from './status-badge';
import { TicketCategorySelect } from './ticket-category-select';
import { TicketNotes } from './ticket-notes';
import { TicketPrioritySelect } from './ticket-priority-select';
import type { TicketPriority } from './ticket-row';
import { TicketStatusMenu } from './ticket-status-menu';
import { TicketTagEditor } from './ticket-tag-editor';

export interface TicketInfoPanelProps { status: TicketStatus; priority: TicketPriority; category: string; tags: string[]; tagSuggestions?: readonly string[]; canUpdate?: boolean; canAddNotes?: boolean; canEditNotes?: boolean; canDeleteNotes?: boolean; composingNote?: boolean; composerDraft?: string; details: string; detailsMode?: MarkdownEditorMode; detailsDirty?: boolean; readerPresentation?: boolean; readerEditing?: boolean; notes?: readonly NoteCardProps[]; editingNoteId?: string; noteDraft?: string; blockedReason?: string; blockedReasonEditing?: boolean; blockedReasonDraft?: string; providerName?: string; updatedLabel?: string }
export function TicketInfoPanel({ status, priority, category, tags, tagSuggestions, canUpdate = true, canAddNotes = true, canEditNotes = true, canDeleteNotes = true, composingNote = false, composerDraft = '', details, detailsMode = 'preview', detailsDirty = false, readerPresentation = false, readerEditing = false, notes = [], editingNoteId, noteDraft, blockedReason = '', blockedReasonEditing = false, blockedReasonDraft = blockedReason, providerName = 'Hot Sheet git', updatedLabel = 'Updated now' }: TicketInfoPanelProps) {
  return <div class="ticket-inspector__content" data-component="ticket-info-panel">
    <section class="ticket-inspector__metadata" aria-label="Ticket metadata">
      <TicketCategorySelect name="inspector-category" value={category} />
      <TicketPrioritySelect name="inspector-priority" value={priority} />
      <div><span>Status</span><span class="ticket-inspector__status-line"><TicketStatusMenu value={status} />{blockedReason && <BlockedBadge />}</span></div>
    </section>
    <section class="ticket-inspector__section ticket-inspector__blocked-section"><header class="ticket-inspector__section-header"><h2>Blocked reason</h2>{!blockedReasonEditing && <button type="button" class="ticket-inspector__text-action" data-action="edit-blocked-reason">{blockedReason ? 'Edit' : 'Block ticket'}</button>}</header>{blockedReasonEditing ? <div class="ticket-inspector__blocked-editor"><textarea name="blocked-reason" aria-label="Blocked reason">{blockedReasonDraft}</textarea></div> : blockedReason ? <div class="ticket-inspector__blocked-surface"><p>{blockedReason}</p></div> : undefined}</section>
    <section class="ticket-inspector__section ticket-inspector__details-section"><header class="ticket-inspector__section-header"><h2>Details</h2></header><div class="ticket-inspector__details-surface"><MarkdownEditor value={details} mode={detailsMode} dirty={detailsDirty} appearance="embedded" showExpand={false} label="Ticket details" editable={!readerPresentation || readerEditing} /></div></section>
    <section class="ticket-inspector__section"><header class="ticket-inspector__section-header"><h2>Tags</h2></header><TicketTagEditor tags={tags} suggestions={tagSuggestions} editable={canUpdate} /></section>
    <TicketNotes notes={notes} editingNoteId={editingNoteId} noteDraft={noteDraft} composing={composingNote} composerDraft={composerDraft} canAdd={canAddNotes} canEdit={canEditNotes} canDelete={canDeleteNotes} readerMode={readerPresentation} readerEditing={readerEditing} />
    <footer class="ticket-inspector__provenance"><span>{providerName}</span><span>{updatedLabel}</span></footer>
  </div>;
}
