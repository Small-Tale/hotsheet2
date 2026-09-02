import { signal } from 'kerfjs';

import { MarkdownEditor, type MarkdownEditorMode } from '../components/markdown-editor';
import { NoteCard, type NoteKind } from '../components/note-card';
import { NoteComposer } from '../components/note-composer';
import type { InspectorTab } from '../components/ticket-inspector';
import { TicketReader } from '../components/ticket-reader';

export const NOTE_DEMO_KINDS: readonly NoteKind[] = ['regular', 'status', 'feedback_needed', 'feedback_draft', 'activity'];
export const noteDemoNotes = signal([
  { id: 'regular', kind: 'regular' as const, author: 'Claude', time: '12 minutes ago', body: 'The shared row now keeps metadata readable at narrow widths.' },
  { id: 'status', kind: 'status' as const, author: 'Hot Sheet', time: '9 minutes ago', body: 'Status changed from Started to Needs Review.' },
  { id: 'feedback', kind: 'feedback_needed' as const, author: 'Codex', time: '4 minutes ago', title: 'Feedback needed', body: 'Should this interaction preserve the current filter when switching projects?' },
  { id: 'draft', kind: 'feedback_draft' as const, author: 'You', time: '2 minutes ago', body: 'Preserve the current filter when switching between related worktrees.' },
  { id: 'activity', kind: 'activity' as const, author: 'Codex', time: 'Now', body: 'Finished the responsive layout pass and browser verification.' },
]);

export function NoteCardDemo() {
  return <section class="note-card-demo" aria-label="NoteCard demo">{noteDemoNotes.value.map(note => <NoteCard {...note} editing={editingNoteId.value === note.id} draft={editingNoteId.value === note.id ? noteDraft.value : undefined} />)}</section>;
}

export const noteComposerValue = signal('Summarize the design decision and link the verification evidence.');
export function NoteComposerDemo() { return <section class="note-card-demo" aria-label="NoteComposer demo"><NoteComposer value={noteComposerValue.value} /></section>; }

export const READER_NOTES = [
  { id: 'reader-status', kind: 'status' as const, author: 'Hot Sheet', time: '1 hour ago', body: 'Status changed from Not started to Started.' },
  { id: 'reader-note', kind: 'regular' as const, author: 'Claude', time: '24 minutes ago', body: 'The reader should preserve a comfortable line length while the [note history](/ux-demo?component=note-card) remains easy to scan.' },
  { id: 'reader-feedback', kind: 'feedback_needed' as const, author: 'Codex', time: '12 minutes ago', body: 'Should the reader keep this response visible while editing details?' },
  { id: 'reader-draft', kind: 'feedback_draft' as const, author: 'You', time: '8 minutes ago', body: 'Yes, keep the response beside the larger editor.' },
  { id: 'reader-activity', kind: 'activity' as const, author: 'Codex', time: 'Now', body: 'Completed the first browser review of the reading surface.' },
];
export const readerNotes = signal(READER_NOTES);
export const readerTab = signal<InspectorTab>('info');
export const readerAttachments = signal([{ id: 'wireframe', name: 'reader-wireframe.png' }, { id: 'notes', name: 'reader-notes.md' }]);
export const editingNoteId = signal<string | undefined>(undefined);
export const noteDraft = signal('');
export const inspectorBlockedReason = signal('');
export const inspectorBlockedReasonDraft = signal(inspectorBlockedReason.value);
export const inspectorBlockedReasonEditing = signal(false);

export function TicketReaderDemo() {
  return <section class="ticket-reader-demo" aria-label="TicketReader demo"><TicketReader slug="HS2-H892P1" title="Build TicketReader component and UX demo" status="started" priority="high" category="feature" tags={['client', 'ux', 'reader']} details={markdownValue.value} detailsMode={markdownMode.value} detailsDirty={markdownValue.value !== markdownSavedValue.value} notes={readerNotes.value} editingNoteId={editingNoteId.value} noteDraft={noteDraft.value} blockedReason={inspectorBlockedReason.value} blockedReasonEditing={inspectorBlockedReasonEditing.value} blockedReasonDraft={inspectorBlockedReasonDraft.value} providerName="Hot Sheet git" updatedLabel="Updated now" activeTab={readerTab.value} attachments={readerAttachments.value} timelineEntries={[{ id: 'started', time: '1h ago', title: 'Development started', subtitle: 'The reader composition work is underway.', emphasized: true }, { id: 'reviewed', time: 'Now', title: 'Reader composition reviewed', subtitle: 'Shared inspector behavior is ready for review.', emphasized: true }]} /></section>;
}

export const MARKDOWN_INITIAL = `## Implementation notes

[Open the component guide](/ux-demo?component=tag-chip).
The editor keeps **source and preview** in one predictable surface. See [CommonMark](https://commonmark.org/) for the base syntax.

- [x] Preserve drafts while switching modes.
- [ ] Validate the final reader flow.

| Surface | Behavior |
| --- | --- |
| Inspector | Compact editing |
| Reader | Full ticket editing |

> Raw HTML is shown as text rather than executed.

Use \`Cmd+Enter\` for a future keyboard save shortcut.`;
export const markdownValue = signal(MARKDOWN_INITIAL);
export const markdownSavedValue = signal(MARKDOWN_INITIAL);
export const markdownMode = signal<MarkdownEditorMode>('preview');
export const markdownExpanded = signal(false);
export const markdownEvent = signal('Edit the source, preview it, or expand the editor.');

export function saveMarkdown(): void { markdownSavedValue.value = markdownValue.value; markdownMode.value = 'preview'; markdownEvent.value = 'Markdown saved.'; }
export function cancelMarkdown(): void { markdownValue.value = markdownSavedValue.value; markdownMode.value = 'preview'; markdownEvent.value = 'Edits cancelled.'; }

export function MarkdownEditorDemo() {
  return <section class="markdown-editor-demo" aria-label="MarkdownEditor demo">
    <MarkdownEditor value={markdownValue.value} mode={markdownMode.value} expanded={markdownExpanded.value} dirty={markdownValue.value !== markdownSavedValue.value} />
    <p class="component-stage__event" aria-live="polite">{markdownEvent.value}</p>
  </section>;
}
