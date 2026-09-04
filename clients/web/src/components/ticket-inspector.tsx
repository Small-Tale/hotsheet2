import '@awesome.me/webawesome/dist/components/button/button.js';
import './ticket-inspector.css';

import { BookOpen, CircleAlert, Info, ListTree, MessageSquareCode, PanelRightClose, Paperclip, Star, X } from 'lucide';

import type { CodeReview } from '../api';
import type { InlineFeedbackReply } from '../feedback-replies';
import type { TicketFieldConflict as TicketFieldConflictState } from '../ticket-field-reconciliation';
import { LucideIcon } from './lucide-icon';
import type { MarkdownEditorMode } from './markdown-editor';
import type { NoteCardProps } from './note-card';
import type { TicketStatus } from './status-badge';
import { type TicketAttachmentItem,TicketAttachments } from './ticket-attachments';
import { TicketCodeReview } from './ticket-code-review';
import { TicketFieldConflict } from './ticket-field-conflict';
import { TicketInfoPanel } from './ticket-info-panel';
import type { TicketPriority } from './ticket-row';
import { TicketTimeline, type TicketTimelineEntry } from './ticket-timeline';
import { Toolbar } from './toolbar';
import { ToolbarControlGroup } from './toolbar-control-group';
import { ToolbarText } from './toolbar-text';

export type InspectorTab = 'info' | 'timeline' | 'code-review' | 'attachments';

export interface TicketInspectorProps {
  slug: string;
  title: string;
  titleEditing?: boolean;
  titleDraft?: string;
  canUpdate?: boolean;
  canAddNotes?: boolean;
  canEditNotes?: boolean;
  canDeleteNotes?: boolean;
  composingNote?: boolean;
  composerDraft?: string;
  status: TicketStatus;
  priority: TicketPriority;
  category: string;
  tags: string[];
  tagSuggestions?: readonly string[];
  details: string;
  detailsMode?: MarkdownEditorMode;
  detailsDirty?: boolean;
  activeTab?: InspectorTab;
  upNext?: boolean;
  upNextEligible?: boolean;
  /** The ticket has an unresolved `feedback_needed` note — it is waiting on the user. */
  feedbackNeeded?: boolean;
  timelineEntries?: readonly TicketTimelineEntry[];
  attachments?: readonly TicketAttachmentItem[];
  codeReview?: CodeReview;
  codeReviewLoading?: boolean;
  codeReviewMessage?: string;
  expandedCodeReviewCommits?: readonly string[];
  attachmentsEnabled?: boolean;
  attachmentMessage?: string;
  notes?: readonly NoteCardProps[];
  editingNoteId?: string;
  noteDraft?: string;
  inlineFeedbackReplies?: Readonly<Record<string, readonly InlineFeedbackReply[]>>;
  blockedReason?: string;
  blockedReasonEditing?: boolean;
  blockedReasonDraft?: string;
  providerName?: string;
  updatedLabel?: string;
  presentation?: 'sidebar' | 'reader';
  fieldConflict?: TicketFieldConflictState;
  fieldConflictResolution?: string;
}

const tabs = [
  { id: 'info', label: 'Info', icon: Info, iconName: 'info' },
  { id: 'timeline', label: 'Timeline', icon: ListTree, iconName: 'list-tree' },
  { id: 'code-review', label: 'Code Review', icon: MessageSquareCode, iconName: 'message-square-code' },
  { id: 'attachments', label: 'Attachments', icon: Paperclip, iconName: 'paperclip' },
] as const;

export function TicketInspector({ slug, title, titleEditing = false, titleDraft = title, canUpdate = true, canAddNotes = true, canEditNotes = true, canDeleteNotes = true, composingNote = false, composerDraft = '', status, priority, category, tags, tagSuggestions, details, detailsMode, detailsDirty, activeTab = 'info', upNext = false, upNextEligible = status === 'not_started' || status === 'started', feedbackNeeded = false, timelineEntries, attachments, codeReview, codeReviewLoading = false, codeReviewMessage = '', expandedCodeReviewCommits, attachmentsEnabled = true, attachmentMessage = '', notes, editingNoteId, noteDraft, inlineFeedbackReplies, blockedReason, blockedReasonEditing, blockedReasonDraft, providerName, updatedLabel, presentation = 'sidebar', fieldConflict, fieldConflictResolution = fieldConflict?.mine ?? '' }: TicketInspectorProps) {
  const star = <>{upNextEligible && <button type="button" class={`ticket-inspector__star${upNext ? ' ticket-inspector__star--active' : ''}`} data-action="toggle-inspector-up-next" aria-label={upNext ? 'Remove from Up Next' : 'Add to Up Next'}><LucideIcon icon={Star} name="star" /></button>}</>;
  const close = <button type="button" data-action={presentation === 'reader' ? 'close-ticket-reader' : 'close-ticket-inspector'} aria-label={presentation === 'reader' ? 'Close ticket reader' : 'Hide inspector'}><LucideIcon icon={presentation === 'reader' ? X : PanelRightClose} name={presentation === 'reader' ? 'x' : 'panel-right-close'} /></button>;
  const actions = presentation === 'reader'
    ? <ToolbarControlGroup appearance="borderless" label="Ticket actions">{star}{close}</ToolbarControlGroup>
    : <ToolbarControlGroup appearance="borderless" label="Ticket actions">{star}<button type="button" data-action="open-ticket-reader" aria-label="Open ticket reader" title="Open ticket reader"><LucideIcon icon={BookOpen} name="book-open" /></button>{close}</ToolbarControlGroup>;
  return <aside class={presentation === 'reader' ? 'ticket-inspector ticket-inspector--reader' : 'ticket-inspector'} data-component="ticket-inspector" data-presentation={presentation} data-ticket-slug={slug} data-needs-review={String(feedbackNeeded)} data-attachment-drop-target="true" aria-label={`${slug} inspector`}>
    <header class="ticket-inspector__header">
      <Toolbar divider={false} center={<button type="button" class="ticket-inspector__slug" data-action="copy-ticket-slug" aria-label={`Copy ticket number ${slug}`} title="Copy ticket number"><ToolbarText text={slug} size="small" /></button>} trailing={actions} />
      {titleEditing ? <input class="ticket-inspector__title-input" name="ticket-title" aria-label="Ticket title" value={titleDraft} /> : <h1 data-action={canUpdate ? 'edit-ticket-title' : undefined} data-editable={String(canUpdate)} tabIndex={canUpdate ? 0 : undefined} title={canUpdate ? 'Double-click to edit title' : undefined}>{title}</h1>}
    </header>
    {feedbackNeeded && <div class="ticket-inspector__feedback" role="status"><LucideIcon icon={CircleAlert} name="circle-alert" class="ticket-inspector__feedback-icon" /><span>Needs review</span></div>}
    {fieldConflict && <TicketFieldConflict conflict={fieldConflict} resolution={fieldConflictResolution} />}
    <nav class="ticket-inspector__tabs" aria-label="Ticket inspector sections">{tabs.map(tab => <button type="button" data-action="set-inspector-tab" data-inspector-tab={tab.id} aria-label={tab.id === 'attachments' && attachments?.length ? `${tab.label}, ${attachments.length}` : tab.label} aria-current={activeTab === tab.id ? 'page' : undefined}><LucideIcon icon={tab.icon} name={tab.iconName} /><span class="ticket-inspector__tab-label">{tab.label}</span>{tab.id === 'attachments' && Boolean(attachments?.length) && <span class="ticket-inspector__tab-count" aria-hidden="true">{attachments!.length}</span>}</button>)}</nav>
    {activeTab === 'info' && <TicketInfoPanel status={status} priority={priority} category={category} tags={tags} tagSuggestions={tagSuggestions} canUpdate={canUpdate} canAddNotes={canAddNotes} canEditNotes={canEditNotes} canDeleteNotes={canDeleteNotes} composingNote={composingNote} composerDraft={composerDraft} details={details} detailsMode={detailsMode} detailsDirty={detailsDirty} readerPresentation={presentation === 'reader'} notes={notes} editingNoteId={editingNoteId} noteDraft={noteDraft} inlineFeedbackReplies={inlineFeedbackReplies} blockedReason={blockedReason} blockedReasonEditing={blockedReasonEditing} blockedReasonDraft={blockedReasonDraft} providerName={providerName} updatedLabel={updatedLabel} />}
    {activeTab === 'timeline' && <TicketTimeline entries={timelineEntries} />}
    {activeTab === 'code-review' && <TicketCodeReview review={codeReview} loading={codeReviewLoading} message={codeReviewMessage} expandedCommits={expandedCodeReviewCommits} />}
    {activeTab === 'attachments' && <TicketAttachments attachments={attachments} enabled={attachmentsEnabled} message={attachmentMessage} />}
  </aside>;
}
