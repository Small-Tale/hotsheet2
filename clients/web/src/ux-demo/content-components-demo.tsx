import { signal } from 'kerfjs';
import { MarkdownEditor, type MarkdownEditorMode } from '../components/markdown-editor';
import { NoteCard, type NoteKind } from '../components/note-card';
import { TicketReader } from '../components/ticket-reader';

export const NOTE_DEMO_KINDS: readonly NoteKind[] = ['regular', 'status', 'feedback_needed', 'activity'];

export function NoteCardDemo() {
  return <section class="note-card-demo" aria-label="NoteCard demo">
    <NoteCard id="regular" kind="regular" author="Claude" time="12 minutes ago" body="The shared row now keeps metadata readable at narrow widths." />
    <NoteCard id="status" kind="status" author="Hot Sheet" time="9 minutes ago" body="Status changed from Started to Needs Review." />
    <NoteCard id="feedback" kind="feedback_needed" author="Codex" time="4 minutes ago" title="Feedback needed" body="Should this interaction preserve the current filter when switching projects?" />
    <NoteCard id="activity" kind="activity" author="Codex" time="Now" body="Finished the responsive layout pass and browser verification." />
  </section>;
}

export const READER_NOTES = [
  { id: 'reader-status', kind: 'status' as const, author: 'Hot Sheet', time: '1 hour ago', body: 'Status changed from Not started to Started.' },
  { id: 'reader-note', kind: 'regular' as const, author: 'Claude', time: '24 minutes ago', body: 'The reader should preserve a comfortable line length while the note history remains easy to scan.' },
  { id: 'reader-activity', kind: 'activity' as const, author: 'Codex', time: 'Now', body: 'Completed the first browser review of the reading surface.' },
];
export const READER_DETAILS = `## Goal
Create a focused reading surface for ticket details and durable notes.

## Acceptance criteria
- Keep long-form content comfortably readable.
- Scroll details and notes together under a stable header.
- Reuse the same NoteCard presentation shown elsewhere.`;

export function TicketReaderDemo() {
  return <section class="ticket-reader-demo" aria-label="TicketReader demo"><TicketReader slug="HS2-H892P1" title="Build TicketReader component and UX demo" details={READER_DETAILS} notes={READER_NOTES} /></section>;
}

export const MARKDOWN_INITIAL = `## Implementation notes
The editor keeps source and preview in one predictable surface.

- Preserve drafts while switching modes.
- Make expanded editing explicit and reversible.`;
export const markdownValue = signal(MARKDOWN_INITIAL);
export const markdownSavedValue = signal(MARKDOWN_INITIAL);
export const markdownMode = signal<MarkdownEditorMode>('write');
export const markdownExpanded = signal(false);
export const markdownEvent = signal('Edit the source, preview it, or expand the editor.');

export function saveMarkdown(): void { markdownSavedValue.value = markdownValue.value; markdownEvent.value = 'Markdown saved.'; }
export function cancelMarkdown(): void { markdownValue.value = markdownSavedValue.value; markdownMode.value = 'write'; markdownEvent.value = 'Edits cancelled.'; }

export function MarkdownEditorDemo() {
  return <section class="markdown-editor-demo" aria-label="MarkdownEditor demo">
    <MarkdownEditor value={markdownValue.value} mode={markdownMode.value} expanded={markdownExpanded.value} dirty={markdownValue.value !== markdownSavedValue.value} />
    <p class="component-stage__event" aria-live="polite">{markdownEvent.value}</p>
  </section>;
}
