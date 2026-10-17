import { Button } from '@kerfjs/ui/button';
import { CatalogExample } from '@kerfjs/ui/catalog';
import { List } from '@kerfjs/ui/list';
import { Select } from '@kerfjs/ui/select';
import { wireContentItems } from '@kerfjs/ui/wire-content-items';
import { delegate, delegateCapture, effect, type SafeHtml, signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import { AIContentLabel } from '../components/ai-content-label';
import { MarkdownEditor, type MarkdownEditorMode } from '../components/markdown-editor';
import { MarkdownPreview, type MarkdownPreviewProps } from '../components/markdown-preview';
import { NoteCard, type NoteCardProps, type NoteKind } from '../components/note-card';
import { NoteComposer } from '../components/note-composer';
import {
  wireMarkdownDetailsEditor,
  wireNoteComposer,
  wireNoteEditor,
} from '../components/ticket-content-editor-interactions';
import {
  wireBlockedReasonEditor,
  wireFeedbackChoices,
  wireTicketTagEditor,
  wireTicketTitleEditor,
} from '../components/ticket-field-editor-interactions';
import type { InspectorTab } from '../components/ticket-inspector';
import { showTicketReaderDialog, TicketReader } from '../components/ticket-reader';
import { addTicketTag, openTicketTagSurface, removeTicketTag } from '../components/ticket-tag-editor';
import { parseFeedbackChoices, updateFeedbackChoiceDocumentSelection } from '../feedback-choices';
import { beginFeedbackPromptEdit } from '../feedback-prompt-edits';
import { DemoEvent } from './components/demo-event';
import { selectDemo, selectedId } from './components/demo-state';
import { DEMO_ACTIONS, DEMO_COMPONENTS } from './demo-actions';
import type { DemoModule, FormControl } from './demo-module';
import { DemoSettingsForm } from './demo-settings-form';
import {
  blockedReasonAutosave,
  markdownAutosave,
  noteAutosave,
  readerTitleAutosave,
  tagsAutosave,
  titleAutosave,
} from './fixtures';
import { syncSettingsControls } from './settings-controls';
import { collectionTickets, recordCollectionEvent, toggleCollectionTicketUpNext } from './ticket-collections-demo';
import { inspectorTags, inspectorTitle, inspectorTitleDraft, inspectorTitleEditing } from './workspace-components-demo';

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
    author: 'Codex',
    time: '9 minutes ago',
    body: 'Status changed from Started to Needs Review.',
    aiAuthored: true,
    aiTool: 'Codex',
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
    id: 'mixed-activity',
    kind: 'activity' as const,
    author: 'Codex',
    time: 'Now',
    body: 'Status changed from Started to Completed\n\nVerified the fix in a browser.',
    aiAuthored: true,
    aiTool: 'Codex',
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
    <>
      <CatalogExample label="Every Note Kind" viewport={{ layout: 'flex-column' }}>
        <List gap="xs">
          {noteDemoNotes.value.map((note) => (
            <NoteCard
              {...note}
              editing={editingNoteId.value === note.id}
              draft={editingNoteId.value === note.id ? noteDraft.value : undefined}
            />
          ))}
        </List>
      </CatalogExample>
      <CatalogExample label='density="compact"' note="The TicketNotes list inset." viewport={{ layout: 'flex-column' }}>
        <List gap="xs">
          {noteDemoNotes.value
            .filter((note) => note.kind === 'regular' || note.kind === 'activity')
            .slice(0, 2)
            .map((note) => (
              <NoteCard {...note} id={`compact-${note.id}`} density="compact" editable={false} deletable={false} />
            ))}
        </List>
      </CatalogExample>
    </>
  );
}

export const noteComposerValue = signal('Summarize the design decision and link the verification evidence.');
export function NoteComposerDemo() {
  return (
    <CatalogExample label="Interactive">
      <NoteComposer value={noteComposerValue.value} />
    </CatalogExample>
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
    id: 'reader-feedback-groups',
    kind: 'feedback_needed' as const,
    author: 'Codex',
    time: '10 minutes ago',
    body: 'A few decisions before I continue.\n\n**1. Which store owns the notes?**\n\nCHOICE:\n- The ticket store\n- The document store\n\n**2. Does the existing "Notes" field stay?**\n\nCHOICE\n\n- Keep it read-only\n- Migrate and remove it\n\n**3. When should the migration run?**\n\nCHOICE:\n\n1. On first launch\n\n2. From an explicit command\n\nAnything else I should know?',
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
/** The reader demo's own ticket title and its title-edit state (HS2-0VFPD5): the reader shows a different
 * demo ticket than the sidebar inspector, so it projects its own editing signals into TicketReader. */
export const READER_TITLE_INITIAL = 'Build TicketReader component and UX demo';
export const readerTitle = signal(READER_TITLE_INITIAL);
export const readerTitleDraft = signal(READER_TITLE_INITIAL);
export const readerTitleEditing = signal(false);
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
    <CatalogExample label="Interactive" note="The reader opens as a dialog over the catalog.">
      <TicketReader
        frameId="ux-demo-ticket-reader"
        open={readerDialogOpen.value}
        slug="HS2-H892P1"
        title={readerTitle.value}
        titleEditing={readerTitleEditing.value}
        titleDraft={readerTitleDraft.value}
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
        providerName="Hot Sheet Git"
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
    </CatalogExample>
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

/** Restore the MarkdownEditor demo's default variants in both state and the live settings controls (HS2-QBR5HC). */
export function resetMarkdownEditorDemo(root: ParentNode): void {
  markdownAppearance.value = 'standalone';
  markdownInset.value = 'padded';
  syncSettingsControls(root, 'markdown-editor', {
    values: { 'markdown-appearance': 'standalone', 'markdown-inset': 'padded' },
  });
}

/** The MarkdownEditor's public appearance and inset variants (HS2-MGVE50). */
export function MarkdownEditorSettings() {
  return (
    <DemoSettingsForm settings="markdown-editor">
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
          { value: 'flush', label: 'Flush (Host Owns the Inset)' },
        ]}
      />
      <Button type="button" action={DEMO_ACTIONS.resetSettings.value}>
        Reset
      </Button>
    </DemoSettingsForm>
  );
}

export function MarkdownEditorDemo() {
  return (
    <CatalogExample label="Interactive">
      <MarkdownEditor
        value={markdownValue.value}
        mode={markdownMode.value}
        expanded={markdownExpanded.value}
        dirty={markdownValue.value !== markdownSavedValue.value}
        appearance={markdownAppearance.value}
        inset={markdownInset.value}
      />
      <DemoEvent>{markdownEvent.value}</DemoEvent>
    </CatalogExample>
  );
}

const PREVIEW_DEMO_SOURCE =
  'The **shared row** keeps metadata readable. See the [component notes](/ux-demo?component=note-card).\n\n- Block rhythm follows the density.\n- Inline `code` keeps its chip.';
// The fragment carries the attachment path, so the preview renders its attachment-image button while
// the demo still loads a real public asset.
const PREVIEW_DEMO_IMAGE =
  '![Wide layout](/ux-gallery-preview.svg#/tickets/HS2-DEMO/attachments/by-name/wide-layout.svg)';

type ToneSurfaceKind = 'muted' | 'accent' | 'heading';
interface PreviewVariant {
  label: string;
  surface?: ToneSurfaceKind;
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

/** A demo-owned container that supplies the color context the inherit/inverse tones are designed for;
 * Kerf's CatalogExample surfaces cover only the default and lowered fills. */
function ToneSurface({ surface, children }: { surface?: ToneSurfaceKind; children: SafeHtml }) {
  return surface ? (
    <div class="demo-tone-surface" data-surface={surface}>
      {children}
    </div>
  ) : (
    children
  );
}

export function MarkdownPreviewDemo() {
  return (
    <>
      {PREVIEW_VARIANTS.map((variant) => (
        <CatalogExample label={variant.label}>
          <ToneSurface surface={variant.surface}>
            <MarkdownPreview source={variant.source ?? PREVIEW_DEMO_SOURCE} {...variant.props} />
          </ToneSurface>
        </CatalogExample>
      ))}
    </>
  );
}

export function AIContentLabelDemo() {
  return (
    <>
      <CatalogExample label="Default (Quiet)" align="glyph">
        <AIContentLabel tool="Claude" />
      </CatalogExample>
      <CatalogExample label="mayContainErrors" align="glyph">
        <AIContentLabel mayContainErrors />
      </CatalogExample>
      <CatalogExample label="With Feedback" align="glyph">
        <AIContentLabel tool="Codex" feedbackTarget="demo:label" />
      </CatalogExample>
      <CatalogExample label='tone="inherit" (Heading Color)'>
        <ToneSurface surface="heading">
          <AIContentLabel tool="Codex" tone="inherit" feedbackTarget="demo:inherit" />
        </ToneSurface>
      </CatalogExample>
    </>
  );
}

export function openDemoTicketReader(root: HTMLElement): void {
  readerDialogOpen.value = false;
  selectDemo('ticket-reader');
  requestAnimationFrame(() => {
    showTicketReaderDialog(root, 'ux-demo-ticket-reader');
    readerDialogOpen.value = true;
  });
}

/** Install this file's demo interactions on the catalog root (HS2-HMVBZW). */
function wireContentComponentsDemo(root: HTMLElement): () => void {
  const demoListeners = createScope();
  // Open the reader whenever its entry is selected, from a deep link or the catalog sidebar, so the
  // stage never shows an empty TicketReader (HS2-G2XKEH).
  demoListeners.add(
    effect(() => {
      if (selectedId.value === 'ticket-reader')
        requestAnimationFrame(() => showTicketReaderDialog(root, 'ux-demo-ticket-reader'));
    }),
  );
  demoListeners.add(
    delegate(root, 'change', '[data-settings="markdown-editor"] [name="markdown-appearance"]', (_event, target) => {
      markdownAppearance.value = (target as FormControl).value as MarkdownEditorAppearanceDemo;
    }),
  );
  demoListeners.add(
    delegate(root, 'change', '[data-settings="markdown-editor"] [name="markdown-inset"]', (_event, target) => {
      markdownInset.value = (target as FormControl).value as MarkdownEditorInsetDemo;
    }),
  );
  // The reader demo edits its own ticket's title; every other surface edits the inspector demo ticket's
  // (HS2-0VFPD5). Each surface projects its own title, draft, and editing signals.
  const titleSurface = (target: Element) =>
    target.closest('[data-component="ticket-reader"]')
      ? { title: readerTitle, draft: readerTitleDraft, editing: readerTitleEditing, autosave: readerTitleAutosave }
      : {
          title: inspectorTitle,
          draft: inspectorTitleDraft,
          editing: inspectorTitleEditing,
          autosave: titleAutosave,
        };
  const beginTitleEdit = (target: Element) => {
    const surface = titleSurface(target),
      host = target.closest('[data-component="ticket-reader"], [data-component="ticket-inspector-header"]') ?? root;
    surface.draft.value = surface.title.value;
    surface.editing.value = true;
    queueMicrotask(() => host.querySelector<HTMLElement>('[name="ticket-title"]')?.focus());
  };
  demoListeners.add(
    wireTicketTitleEditor(root, {
      begin: beginTitleEdit,
      input: (value, field) => {
        const surface = titleSurface(field);
        surface.draft.value = value;
        if (value.trim()) surface.autosave.schedule(value);
      },
      finish: (field) => {
        const surface = titleSurface(field);
        if (!surface.draft.value.trim()) return;
        void surface.autosave.flush().then(() => {
          surface.editing.value = false;
        });
      },
    }),
  );
  // The Add tag surface opens through Kerf's AnchoredSurface on document.body, outside the demo
  // root, so the tag editor's field listeners delegate from the body (HS2-Q4JE3V).
  let tagSurface: ReturnType<typeof openTicketTagSurface> | undefined;
  demoListeners.add(() => tagSurface?.close());
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.openTicketTagPopover.selector, (_event, target) => {
      tagSurface?.close();
      tagSurface = openTicketTagSurface(target);
    }),
  );
  demoListeners.add(
    wireTicketTagEditor(document.body, {
      add: (value) => {
        inspectorTags.value = addTicketTag(inspectorTags.value, value);
        tagsAutosave.schedule(inspectorTags.value);
      },
      flush: () => {
        void tagsAutosave.flush();
      },
      remove: (tag) => {
        inspectorTags.value = removeTicketTag(inspectorTags.value, tag);
        tagsAutosave.schedule(inspectorTags.value);
      },
    }),
  );
  const beginBlockedReasonEdit = () => {
    inspectorBlockedReasonDraft.value = inspectorBlockedReason.value;
    inspectorBlockedReasonEditing.value = true;
    queueMicrotask(() => root.querySelector<HTMLElement>('[name="blocked-reason"]')?.focus());
  };
  demoListeners.add(
    wireBlockedReasonEditor(root, {
      begin: beginBlockedReasonEdit,
      input: (value) => {
        inspectorBlockedReasonDraft.value = value;
        blockedReasonAutosave.schedule(value);
      },
      finish: () => {
        void blockedReasonAutosave.flush().then(() => {
          inspectorBlockedReasonEditing.value = false;
          recordCollectionEvent('Blocked reason autosaved');
        });
      },
    }),
  );
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.toggleInspectorUpNext.selector, () => {
      const ticket = collectionTickets.value.find((item) => item.selected) ?? collectionTickets.value[0];
      toggleCollectionTicketUpNext(ticket.slug);
    }),
  );
  // The production NoteComposer and NoteCard handlers with fixture side effects (HS2-KM3DNM).
  demoListeners.add(
    wireNoteComposer(root, {
      open: () => {
        recordCollectionEvent('Note composer requested');
      },
      setDraft: (value) => {
        noteComposerValue.value = value;
      },
      cancel: () => {
        noteComposerValue.value = '';
        recordCollectionEvent('New note cancelled');
      },
      submit: () => {
        if (noteComposerValue.value.trim()) recordCollectionEvent('New note submitted');
      },
    }),
  );
  function beginDemoNoteEdit(id: string) {
    editingNoteId.value = id;
    noteDraft.value =
      readerNotes.value.find((note) => note.id === id)?.body ??
      noteDemoNotes.value.find((note) => note.id === id)?.body ??
      '';
    queueMicrotask(() => root.querySelector<HTMLElement>(`[name="note-body"][data-note-id="${id}"]`)?.focus());
  }
  demoListeners.add(
    wireNoteEditor(root, {
      begin: (id) => {
        beginDemoNoteEdit(id);
      },
      beginPrompt: (id) => {
        beginFeedbackPromptEdit(id);
        beginDemoNoteEdit(id);
      },
      input: (field, id, response) => {
        editingNoteId.value = id;
        noteDraft.value = field.value;
        if (!response && editingNoteId.value)
          noteAutosave.schedule({ id: editingNoteId.value, value: noteDraft.value });
      },
      finish: () => {
        void noteAutosave.flush().then(() => {
          editingNoteId.value = undefined;
          noteDraft.value = '';
          recordCollectionEvent('Note autosaved');
        });
      },
      save: (target) => {
        const id = editingNoteId.value ?? target.dataset.noteId;
        if (id) {
          const original = readerNotes.value.find((note) => note.id === id);
          if (target.dataset.noteResponse === 'true')
            readerNotes.value = [
              ...readerNotes.value,
              {
                id: `${id}-response`,
                kind: 'regular',
                author: 'You',
                time: 'Now',
                body: noteDraft.value,
              },
            ];
          else
            readerNotes.value = readerNotes.value.map((note) =>
              note.id === id
                ? {
                    ...note,
                    kind: original?.kind === 'feedback_draft' ? ('regular' as const) : note.kind,
                    body: noteDraft.value,
                  }
                : note,
            );
          noteDemoNotes.value = noteDemoNotes.value.map((note) =>
            note.id === id ? { ...note, body: noteDraft.value } : note,
          );
        }
        editingNoteId.value = undefined;
        noteDraft.value = '';
        recordCollectionEvent('Note edit saved');
      },
    }),
  );
  let demoFeedbackChoiceAnchor: string | undefined = 'choice-1';
  // Feedback choices are Kerf multi-select ContentItems; Kerf turns Enter/Space into their click (HS2-3XV34B).
  demoListeners.add(wireContentItems(root));
  demoListeners.add(
    wireFeedbackChoices(root, {
      toggle: (noteId, choiceId, modifiers) => {
        const note = readerNotes.value.find((item) => item.id === noteId),
          choices = note && parseFeedbackChoices(note.body);
        if (!choices) return;
        const next = updateFeedbackChoiceDocumentSelection(
          choices,
          readerFeedbackChoiceSelections.value[noteId] ?? [],
          choiceId,
          demoFeedbackChoiceAnchor,
          modifiers,
        );
        readerFeedbackChoiceSelections.value = { ...readerFeedbackChoiceSelections.value, [noteId]: next.selected };
        demoFeedbackChoiceAnchor = next.anchor;
        recordCollectionEvent(
          `${next.selected.length} feedback choice${next.selected.length === 1 ? '' : 's'} selected`,
        );
      },
    }),
  );
  demoListeners.add(
    delegate(root, 'click', '[data-action="open-ticket-reader"], [data-action="respond-to-feedback"]', () => {
      recordCollectionEvent('Ticket reader requested');
      openDemoTicketReader(root);
    }),
  );
  // The production MarkdownEditor handlers with fixture side effects (HS2-KM3DNM).
  demoListeners.add(
    wireMarkdownDetailsEditor(root, {
      begin: () => {
        markdownMode.value = 'write';
        queueMicrotask(() => root.querySelector<HTMLElement>('[name="markdown-source"]')?.focus());
      },
      input: (field) => {
        markdownValue.value = field.value;
        markdownEvent.value = 'Saving changes…';
        markdownAutosave.schedule(markdownValue.value);
      },
      focusLeave: (_field, _next, insideEditor) => {
        if (insideEditor) return;
        setTimeout(
          () =>
            void markdownAutosave.flush().then(() => {
              markdownMode.value = 'preview';
            }),
          0,
        );
      },
      toggleExpanded: () => {
        markdownExpanded.value = !markdownExpanded.value;
        markdownEvent.value = markdownExpanded.value ? 'Expanded editor opened.' : 'Inline editor restored.';
      },
    }),
  );
  demoListeners.add(
    delegateCapture(root, 'wa-after-hide', DEMO_COMPONENTS.ticketReader.selector, () => {
      readerDialogOpen.value = false;
      selectDemo('ticket-info-panel');
    }),
  );
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.toggleReaderTextSize.selector, () => {
      readerLargeText.value = !readerLargeText.value;
    }),
  );
  return () => {
    demoListeners.dispose();
  };
}

/** The catalog entries this file renders (HS2-HMVBZW). */
export const demoModule: DemoModule = {
  entries: {
    'note-card': { catalogExamples: true, Demo: NoteCardDemo },
    'note-composer': { catalogExamples: true, Demo: NoteComposerDemo },
    'ticket-reader': { catalogExamples: true, Demo: TicketReaderDemo },
    'markdown-editor': {
      catalogExamples: true,
      Demo: MarkdownEditorDemo,
      Settings: MarkdownEditorSettings,
      reset: resetMarkdownEditorDemo,
    },
    'markdown-preview': { catalogExamples: true, Demo: MarkdownPreviewDemo },
    'ai-content-label': { catalogExamples: true, Demo: AIContentLabelDemo },
  },
  wire: wireContentComponentsDemo,
};
