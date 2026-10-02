import { Select } from '@kerfjs/ui/select';
import { signal } from 'kerfjs';

import { AIContentLabel } from '../components/ai-content-label';
import { MarkdownEditor, type MarkdownEditorMode } from '../components/markdown-editor';
import { MarkdownPreview, type MarkdownPreviewProps } from '../components/markdown-preview';
import { NoteCard, type NoteCardProps, type NoteKind } from '../components/note-card';
import { NoteComposer } from '../components/note-composer';
import type { InspectorTab } from '../components/ticket-inspector';
import { TicketReader } from '../components/ticket-reader';

export const NOTE_DEMO_KINDS: readonly NoteKind[] = [
  'regular',
  'status',
  'feedback_needed',
  'feedback_draft',
  'activity',
];
export const noteDemoNotes = signal<NoteCardProps[]>([
  {
    id: 'regular',
    kind: 'regular' as const,
    author: 'Claude',
    time: '12 minutes ago',
    body: 'The shared row now keeps metadata readable at narrow widths.',
    aiAuthored: true,
    aiTool: 'Claude',
    aiMayContainErrors: false,
    confidence: 82,
  },
  {
    id: 'status',
    kind: 'status' as const,
    author: 'Hot Sheet',
    time: '9 minutes ago',
    body: 'Status changed from Started to Needs Review.',
  },
  {
    id: 'feedback',
    kind: 'feedback_needed' as const,
    author: 'Codex',
    time: '4 minutes ago',
    title: 'Feedback needed',
    body: 'Which behavior should we preserve?\n\n1. Keep the current filter\n2. Reset to the queue\n3. Remember per project',
  },
  {
    id: 'draft',
    kind: 'feedback_draft' as const,
    author: 'You',
    time: '2 minutes ago',
    body: 'Preserve the current filter when switching between related worktrees.',
  },
  {
    id: 'activity',
    kind: 'activity' as const,
    author: 'Codex',
    time: 'Now',
    body: 'Finished the responsive layout pass and browser verification.',
    aiAuthored: true,
    aiTool: 'Codex',
    confidence: 96,
  },
  {
    id: 'scored-partial',
    kind: 'regular' as const,
    author: 'Claude',
    time: 'Now',
    body: 'Completed with unit coverage; the narrow-layout pass was not run in a browser.',
    aiAuthored: true,
    aiTool: 'Claude',
    aiMayContainErrors: false,
    confidence: 55,
  },
  {
    id: 'scored-low',
    kind: 'regular' as const,
    author: 'Codex',
    time: 'Now',
    body: 'Shipped without end-to-end verification; the gaps are filed as follow-ups.',
    aiAuthored: true,
    aiTool: 'Codex',
    confidence: 32,
  },
]);

/** Demo stand-in for the server-derived `latest_confidence`: the newest scored note. */
export function latestDemoConfidence(notes: readonly NoteCardProps[]): number | undefined {
  return [...notes].reverse().find((note) => note.confidence !== undefined)?.confidence;
}

export function NoteCardDemo() {
  return (
    <section class="note-card-demo" aria-label="NoteCard demo">
      {noteDemoNotes.value.map((note) => (
        <NoteCard
          {...note}
          editing={editingNoteId.value === note.id}
          draft={editingNoteId.value === note.id ? noteDraft.value : undefined}
        />
      ))}
      <p class="note-card-demo__caption">density="compact" — the TicketNotes list inset</p>
      {noteDemoNotes.value
        .filter((note) => note.kind === 'regular' || note.kind === 'activity')
        .slice(0, 2)
        .map((note) => (
          <NoteCard {...note} id={`compact-${note.id}`} density="compact" editable={false} deletable={false} />
        ))}
    </section>
  );
}

export const noteComposerValue = signal('Summarize the design decision and link the verification evidence.');
export function NoteComposerDemo() {
  return (
    <section class="note-card-demo" aria-label="NoteComposer demo">
      <NoteComposer value={noteComposerValue.value} />
    </section>
  );
}

export const READER_NOTES: NoteCardProps[] = [
  {
    id: 'reader-status',
    kind: 'status' as const,
    author: 'Hot Sheet',
    time: '1 hour ago',
    body: 'Status changed from Not started to Started.',
  },
  {
    id: 'reader-note',
    kind: 'regular' as const,
    author: 'Claude',
    time: '24 minutes ago',
    body: 'The reader should preserve a comfortable line length while the [note history](/ux-demo?component=note-card) remains easy to scan.',
    aiAuthored: true,
    aiTool: 'Claude',
    aiMayContainErrors: false,
    confidence: 88,
  },
  {
    id: 'reader-feedback',
    kind: 'feedback_needed' as const,
    author: 'Codex',
    time: '12 minutes ago',
    body: 'How should the reader preserve this response?\n\nCHOICE:\n- Keep it beside the **larger editor**\n- Move it below the editor\n- `attachment:reader-wireframe.png`',
  },
  {
    id: 'reader-draft',
    kind: 'feedback_draft' as const,
    author: 'You',
    time: '8 minutes ago',
    body: 'Yes, keep the response beside the larger editor.',
  },
  {
    id: 'reader-activity',
    kind: 'activity' as const,
    author: 'Codex',
    time: 'Now',
    body: 'Completed the first browser review of the reading surface.',
    aiAuthored: true,
    aiTool: 'Codex',
  },
];
export const readerNotes = signal(READER_NOTES);
export const readerFeedbackChoiceSelections = signal<Record<string, string[]>>({ 'reader-feedback': ['choice-1'] });
export const readerTab = signal<InspectorTab>('info');
export const readerLargeText = signal(false);
export const readerDialogOpen = signal(true);
export const readerAttachments = signal([
  { id: 'wireframe', name: 'reader-wireframe.png' },
  { id: 'notes', name: 'reader-notes.md' },
]);
export const editingNoteId = signal<string | undefined>(undefined);
export const noteDraft = signal('');
export const inspectorBlockedReason = signal('');
export const inspectorBlockedReasonDraft = signal(inspectorBlockedReason.value);
export const inspectorBlockedReasonEditing = signal(false);

export function TicketReaderDemo() {
  return (
    <section class="ticket-reader-demo" aria-label="TicketReader demo">
      <TicketReader
        frameId="ux-demo-ticket-reader"
        open={readerDialogOpen.value}
        slug="HS2-H892P1"
        title="Build TicketReader component and UX demo"
        status="started"
        priority="high"
        category="feature"
        tags={['client', 'ux', 'reader']}
        details={markdownValue.value}
        detailsMode={markdownMode.value}
        detailsDirty={markdownValue.value !== markdownSavedValue.value}
        notes={readerNotes.value}
        editingNoteId={editingNoteId.value}
        noteDraft={noteDraft.value}
        feedbackChoiceSelections={readerFeedbackChoiceSelections.value}
        blockedReason={inspectorBlockedReason.value}
        blockedReasonEditing={inspectorBlockedReasonEditing.value}
        blockedReasonDraft={inspectorBlockedReasonDraft.value}
        providerName="Hot Sheet git"
        updatedLabel="Updated now"
        activeTab={readerTab.value}
        attachments={readerAttachments.value}
        attachmentContext={{ checkout: 'ux-demo', ticket: 'HS2-H892P1' }}
        timelineEntries={[
          {
            id: 'started',
            time: '1h ago',
            title: 'Development started',
            subtitle: 'The reader composition work is underway.',
            emphasized: true,
          },
          {
            id: 'reviewed',
            time: 'Now',
            title: 'Reader composition reviewed',
            subtitle: 'Shared inspector behavior is ready for review.',
            emphasized: true,
          },
        ]}
        largeText={readerLargeText.value}
      />
    </section>
  );
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

export function saveMarkdown(): void {
  markdownSavedValue.value = markdownValue.value;
  markdownMode.value = 'preview';
  markdownEvent.value = 'Markdown saved.';
}
export function cancelMarkdown(): void {
  markdownValue.value = markdownSavedValue.value;
  markdownMode.value = 'preview';
  markdownEvent.value = 'Edits cancelled.';
}

export type MarkdownEditorAppearanceDemo = 'standalone' | 'embedded';
export type MarkdownEditorInsetDemo = 'padded' | 'flush';
export const markdownAppearance = signal<MarkdownEditorAppearanceDemo>('standalone');
export const markdownInset = signal<MarkdownEditorInsetDemo>('padded');

/** The MarkdownEditor's public appearance and inset variants (HS2-MGVE50). */
export function MarkdownEditorSettings() {
  return (
    <form class="settings-form" data-settings="markdown-editor">
      <Select
        name="markdown-appearance"
        label="Appearance"
        value={markdownAppearance.value}
        choices={[
          { value: 'standalone', label: 'Standalone' },
          { value: 'embedded', label: 'Embedded' },
        ]}
      />
      <Select
        name="markdown-inset"
        label="Inset"
        value={markdownInset.value}
        choices={[
          { value: 'padded', label: 'Padded' },
          { value: 'flush', label: 'Flush (host owns the inset)' },
        ]}
      />
    </form>
  );
}

export function MarkdownEditorDemo() {
  return (
    <section class="markdown-editor-demo" aria-label="MarkdownEditor demo">
      <MarkdownEditor
        value={markdownValue.value}
        mode={markdownMode.value}
        expanded={markdownExpanded.value}
        dirty={markdownValue.value !== markdownSavedValue.value}
        appearance={markdownAppearance.value}
        inset={markdownInset.value}
      />
      <p class="component-stage__event" aria-live="polite">
        {markdownEvent.value}
      </p>
    </section>
  );
}

const PREVIEW_DEMO_SOURCE =
  'The **shared row** keeps metadata readable. See the [component notes](/ux-demo?component=note-card).\n\n- Block rhythm follows the density.\n- Inline `code` keeps its chip.';
// The fragment carries the attachment path, so the preview renders its attachment-image button while
// the demo still loads a real public asset.
const PREVIEW_DEMO_IMAGE =
  '![Wide layout](/ux-gallery-preview.svg#/tickets/HS2-DEMO/attachments/by-name/wide-layout.svg)';

interface PreviewVariant {
  label: string;
  surface?: 'muted' | 'accent';
  source?: string;
  props: Partial<Pick<MarkdownPreviewProps, 'tone' | 'size' | 'density' | 'media'>>;
}

/** Every public MarkdownPreview presentation variant, each on the surface it is designed for. */
export const PREVIEW_VARIANTS: readonly PreviewVariant[] = [
  { label: 'Default', props: {} },
  { label: 'tone="inherit" (muted container)', surface: 'muted', props: { tone: 'inherit' } },
  { label: 'tone="inverse" (brand fill)', surface: 'accent', props: { tone: 'inverse' } },
  { label: 'size="small"', props: { size: 'small' } },
  { label: 'size="inherit"', surface: 'muted', props: { size: 'inherit' } },
  { label: 'density="compact"', props: { density: 'compact' } },
  { label: 'density="flush"', props: { density: 'flush' } },
  { label: 'media="full"', source: PREVIEW_DEMO_IMAGE, props: {} },
  { label: 'media="thumbnail"', source: PREVIEW_DEMO_IMAGE, props: { media: 'thumbnail' } },
  { label: 'Empty', source: '', props: {} },
];

export function MarkdownPreviewDemo() {
  return (
    <section class="markdown-preview-demo" aria-label="MarkdownPreview demo">
      {PREVIEW_VARIANTS.map((variant) => (
        <figure class="markdown-preview-demo__variant">
          <figcaption>{variant.label}</figcaption>
          <div class="markdown-preview-demo__surface" data-surface={variant.surface}>
            <MarkdownPreview source={variant.source ?? PREVIEW_DEMO_SOURCE} {...variant.props} />
          </div>
        </figure>
      ))}
    </section>
  );
}

export function AIContentLabelDemo() {
  return (
    <section class="markdown-preview-demo" aria-label="AIContentLabel demo">
      <figure class="markdown-preview-demo__variant">
        <figcaption>Default (quiet)</figcaption>
        <div class="markdown-preview-demo__surface">
          <AIContentLabel tool="Claude" />
        </div>
      </figure>
      <figure class="markdown-preview-demo__variant">
        <figcaption>mayContainErrors</figcaption>
        <div class="markdown-preview-demo__surface">
          <AIContentLabel mayContainErrors />
        </div>
      </figure>
      <figure class="markdown-preview-demo__variant">
        <figcaption>With feedback</figcaption>
        <div class="markdown-preview-demo__surface">
          <AIContentLabel tool="Codex" feedbackTarget="demo:label" />
        </div>
      </figure>
      <figure class="markdown-preview-demo__variant">
        <figcaption>tone="inherit" (heading color)</figcaption>
        <div class="markdown-preview-demo__surface" data-surface="heading">
          <AIContentLabel tool="Codex" tone="inherit" feedbackTarget="demo:inherit" />
        </div>
      </figure>
    </section>
  );
}
