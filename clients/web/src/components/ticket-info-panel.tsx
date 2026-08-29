import '@awesome.me/webawesome/dist/components/button/button.js';
import type { TicketStatus } from './status-badge';
import { TicketStatusMenu } from './ticket-status-menu';
import { TagChip } from './tag-chip';
import { TicketCategorySelect } from './ticket-category-select';
import { TicketPrioritySelect } from './ticket-priority-select';
import type { TicketPriority } from './ticket-row';
import { MarkdownEditor, type MarkdownEditorMode } from './markdown-editor';
import { TicketNotes } from './ticket-notes';
import type { NoteCardProps } from './note-card';
import { BlockedBadge } from './status-badge';
import './ticket-inspector-panel.css';

export interface TicketInfoPanelProps { status: TicketStatus; priority: TicketPriority; category: string; tags: string[]; details: string; detailsMode?: MarkdownEditorMode; detailsDirty?: boolean; readerPresentation?: boolean; notes?: readonly NoteCardProps[]; editingNoteId?: string; noteDraft?: string; blockedReason?: string; blockedReasonEditing?: boolean; blockedReasonDraft?: string; providerName?: string; updatedLabel?: string }
export function TicketInfoPanel({ status, priority, category, tags, details, detailsMode = 'preview', detailsDirty = false, readerPresentation = false, notes = [], editingNoteId, noteDraft, blockedReason = '', blockedReasonEditing = false, blockedReasonDraft = blockedReason, providerName = 'Hot Sheet git', updatedLabel = 'Updated now' }: TicketInfoPanelProps) {
  return <div class="ticket-inspector__content" data-component="ticket-info-panel">
    <section class="ticket-inspector__metadata" aria-label="Ticket metadata">
      <TicketCategorySelect name="inspector-category" value={category} />
      <TicketPrioritySelect name="inspector-priority" value={priority} />
      <div><span>Status</span><span class="ticket-inspector__status-line"><TicketStatusMenu value={status} />{blockedReason && <BlockedBadge />}</span></div>
    </section>
    <section class="ticket-inspector__section ticket-inspector__blocked-section"><header class="ticket-inspector__section-header"><h2>Blocked reason</h2>{!blockedReasonEditing && <button type="button" class="ticket-inspector__text-action" data-action="edit-blocked-reason">{blockedReason ? 'Edit' : 'Block ticket'}</button>}</header>{blockedReasonEditing ? <div class="ticket-inspector__blocked-editor"><textarea name="blocked-reason" aria-label="Blocked reason">{blockedReasonDraft}</textarea><div><wa-button appearance="plain" data-action="cancel-blocked-reason">Cancel</wa-button><wa-button variant="brand" data-action="save-blocked-reason" disabled={!blockedReasonDraft.trim()}>Save</wa-button></div></div> : blockedReason ? <div class="ticket-inspector__blocked-surface"><p>{blockedReason}</p></div> : undefined}</section>
    <section class="ticket-inspector__section ticket-inspector__details-section"><header class="ticket-inspector__section-header"><h2>Details</h2></header><div class="ticket-inspector__details-surface"><MarkdownEditor value={details} mode={detailsMode} dirty={detailsDirty} appearance="embedded" showExpand={false} label="Ticket details" /></div></section>
    <section class="ticket-inspector__section"><header class="ticket-inspector__section-header"><h2>Tags</h2></header><div class="ticket-inspector__tags">{tags.map((tag, index) => TagChip({ id: `inspector-tag-${index}`, label: tag }))}</div></section>
    <TicketNotes notes={notes} editingNoteId={editingNoteId} noteDraft={noteDraft} />
    <footer class="ticket-inspector__provenance"><span>{providerName}</span><span>{updatedLabel}</span></footer>
  </div>;
}
