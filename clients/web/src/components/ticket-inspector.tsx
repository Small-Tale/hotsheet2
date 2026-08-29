import '@awesome.me/webawesome/dist/components/button/button.js';
import { BookOpen, Info, ListTree, PanelRightClose, Paperclip, Star, X } from 'lucide';
import { LucideIcon } from './lucide-icon';
import type { TicketStatus } from './status-badge';
import type { TicketPriority } from './ticket-row';
import { TicketInfoPanel } from './ticket-info-panel';
import { TicketTimeline, type TicketTimelineEntry } from './ticket-timeline';
import { TicketAttachments, type TicketAttachmentItem } from './ticket-attachments';
import { ToolbarControlGroup } from './toolbar-control-group';
import { ToolbarText } from './toolbar-text';
import { Toolbar } from './toolbar';
import './ticket-inspector.css';
import type { NoteCardProps } from './note-card';
import type { MarkdownEditorMode } from './markdown-editor';

export type InspectorTab = 'info' | 'timeline' | 'attachments';

export interface TicketInspectorProps {
  slug: string;
  title: string;
  status: TicketStatus;
  priority: TicketPriority;
  category: string;
  tags: string[];
  details: string;
  detailsMode?: MarkdownEditorMode;
  detailsDirty?: boolean;
  activeTab?: InspectorTab;
  upNext?: boolean;
  timelineEntries?: readonly TicketTimelineEntry[];
  attachments?: readonly TicketAttachmentItem[];
  notes?: readonly NoteCardProps[];
  editingNoteId?: string;
  noteDraft?: string;
  blockedReason?: string;
  providerName?: string;
  updatedLabel?: string;
  presentation?: 'sidebar' | 'reader';
}

const tabs = [
  { id: 'info', label: 'Info', icon: Info, iconName: 'info' },
  { id: 'timeline', label: 'Timeline', icon: ListTree, iconName: 'list-tree' },
  { id: 'attachments', label: 'Attachments', icon: Paperclip, iconName: 'paperclip' },
] as const;

export function TicketInspector({ slug, title, status, priority, category, tags, details, detailsMode, detailsDirty, activeTab = 'info', upNext = false, timelineEntries, attachments, notes, editingNoteId, noteDraft, blockedReason, providerName, updatedLabel, presentation = 'sidebar' }: TicketInspectorProps) {
  return <aside class={presentation === 'reader' ? 'ticket-inspector ticket-inspector--reader' : 'ticket-inspector'} data-component="ticket-inspector" data-presentation={presentation} data-ticket-slug={slug} data-attachment-drop-target="true" aria-label={`${slug} inspector`}>
    <header class="ticket-inspector__header">
      <Toolbar divider={false} center={<ToolbarText text={slug} size="small" />} trailing={<ToolbarControlGroup appearance="borderless" label="Ticket actions"><button type="button" class={`ticket-inspector__star${upNext ? ' ticket-inspector__star--active' : ''}`} data-action="toggle-inspector-up-next" aria-label={upNext ? 'Remove from Up Next' : 'Add to Up Next'}><LucideIcon icon={Star} name="star" /></button>{presentation === 'reader' ? undefined : <button type="button" data-action="open-ticket-reader" aria-label="Open ticket reader" title="Open ticket reader"><LucideIcon icon={BookOpen} name="book-open" /></button>}<button type="button" data-action={presentation === 'reader' ? 'close-ticket-reader' : 'close-ticket-inspector'} aria-label={presentation === 'reader' ? 'Close ticket reader' : 'Hide inspector'}><LucideIcon icon={presentation === 'reader' ? X : PanelRightClose} name={presentation === 'reader' ? 'x' : 'panel-right-close'} /></button></ToolbarControlGroup>} />
      <h1>{title}</h1>
    </header>
    <nav class="ticket-inspector__tabs" aria-label="Ticket inspector sections">{tabs.map(tab => <button type="button" data-action="set-inspector-tab" data-inspector-tab={tab.id} aria-label={tab.label} aria-current={activeTab === tab.id ? 'page' : undefined}><LucideIcon icon={tab.icon} name={tab.iconName} /><span>{tab.label}</span></button>)}</nav>
    {activeTab === 'info' && <TicketInfoPanel status={status} priority={priority} category={category} tags={tags} details={details} detailsMode={detailsMode} detailsDirty={detailsDirty} readerPresentation={presentation === 'reader'} notes={notes} editingNoteId={editingNoteId} noteDraft={noteDraft} blockedReason={blockedReason} providerName={providerName} updatedLabel={updatedLabel} />}
    {activeTab === 'timeline' && <TicketTimeline entries={timelineEntries} />}
    {activeTab === 'attachments' && <TicketAttachments attachments={attachments} />}
  </aside>;
}
