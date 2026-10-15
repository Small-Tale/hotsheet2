import { attr, delegate, delegateCapture } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import { INSPECTOR_AND_EDITOR_ACTIONS, INSPECTOR_AND_EDITOR_TARGETS } from '../interaction-attrs/inspector-and-editor';
import { clickBeginsMarkdownEdit, keyBeginsMarkdownEdit, repeatPressWouldLeaveNewEditor } from '../markdown-click-edit';

/** MarkdownEditor's default expand toggle (`expandAction`). */
const TOGGLE_MARKDOWN_EXPANDED = attr('data-action', 'toggle-markdown-expanded');

/**
 * Side effects for the ticket content editors (HS2-KM3DNM): the NoteComposer, NoteCard inline note
 * editing, and the MarkdownEditor details editor. Production passes the inspector, reader, and linked
 * reader-frame state with their autosaves; the UX demo passes fixture callbacks, so both resolve the
 * same edit entry, draft, focus-leave, and submit gestures.
 */
export interface NoteComposerBindings {
  readonly open: () => void;
  readonly setDraft: (value: string) => void;
  readonly cancel: () => void;
  /** The composer form was submitted; the helper has already prevented navigation. */
  readonly submit: () => void;
}

/** Wire every NoteComposer under `root`. */
export function wireNoteComposer(root: HTMLElement, bindings: NoteComposerBindings): () => void {
  const lifetime = createScope();
  lifetime.add(
    delegate(root, 'click', INSPECTOR_AND_EDITOR_ACTIONS.addTicketNote.selector, () => {
      bindings.open();
    }),
  );
  lifetime.add(
    delegate(root, 'input', INSPECTOR_AND_EDITOR_TARGETS.newNoteBodyField.selector, (_event, target) => {
      bindings.setDraft((target as HTMLTextAreaElement).value);
    }),
  );
  lifetime.add(
    delegate(root, 'click', INSPECTOR_AND_EDITOR_ACTIONS.cancelNewNote.selector, () => {
      bindings.cancel();
    }),
  );
  lifetime.add(
    delegate(root, 'submit', INSPECTOR_AND_EDITOR_ACTIONS.createNoteForm.selector, (event) => {
      event.preventDefault();
      bindings.submit();
    }),
  );
  return () => {
    lifetime.dispose();
  };
}

export interface NoteEditorBindings {
  /** A click or Enter/Space on a rendered note body began editing note `noteId`. */
  readonly begin: (noteId: string, target: HTMLElement) => void;
  /**
   * The explicit Edit Note action, or a double-click on the prompt body, opened a FEEDBACK NEEDED
   * prompt for editing (HS2-ZXGEFE). Replies still go through the response editor.
   */
  readonly beginPrompt: (noteId: string, target: HTMLElement) => void;
  /** A keystroke in a note editor; `response` marks a feedback reply rather than an edit. */
  readonly input: (field: HTMLTextAreaElement, noteId: string | undefined, response: boolean) => void;
  /** Focus left a note edit (never a feedback reply): write it once and close it. */
  readonly finish: (field: HTMLTextAreaElement) => void;
  /** The explicit save of a note edit or feedback reply. */
  readonly save: (button: HTMLElement) => void;
}

/** Wire NoteCard inline edit entry, drafting, focus-leave autosave, and save under `root`. */
export function wireNoteEditor(root: HTMLElement, bindings: NoteEditorBindings): () => void {
  const lifetime = createScope(),
    begin = (target: Element) => {
      const note = target.closest<HTMLElement>('[data-note-id]');
      if (note?.dataset.noteId) bindings.begin(note.dataset.noteId, target as HTMLElement);
    };
  lifetime.add(
    delegate(root, 'click', INSPECTOR_AND_EDITOR_TARGETS.editOnClick.selector, (event, target) => {
      if (clickBeginsMarkdownEdit(event as MouseEvent, target)) begin(target);
    }),
  );
  lifetime.add(
    delegate(root, 'keydown', INSPECTOR_AND_EDITOR_TARGETS.editOnClick.selector, (event, target) => {
      if (!keyBeginsMarkdownEdit(event as KeyboardEvent, target)) return;
      event.preventDefault();
      begin(target);
    }),
  );
  const beginPrompt = (target: Element) => {
    const note = target.closest<HTMLElement>('[data-note-id]');
    if (note?.dataset.noteId) bindings.beginPrompt(note.dataset.noteId, target as HTMLElement);
  };
  lifetime.add(
    delegate(root, 'click', INSPECTOR_AND_EDITOR_ACTIONS.editFeedbackPrompt.selector, (_event, target) => {
      beginPrompt(target);
    }),
  );
  lifetime.add(
    delegate(root, 'dblclick', INSPECTOR_AND_EDITOR_TARGETS.feedbackPromptBody.selector, (event, target) => {
      // A link or control inside the rendered prompt keeps its own action.
      if ((event.target as Element).closest('a, button, input, textarea, [role="button"]')) return;
      beginPrompt(target);
    }),
  );
  lifetime.add(
    delegate(root, 'input', INSPECTOR_AND_EDITOR_TARGETS.noteBodyField.selector, (_event, target) => {
      const field = target as HTMLTextAreaElement;
      bindings.input(field, field.dataset.noteId, field.dataset.noteResponse === 'true');
    }),
  );
  lifetime.add(
    delegate(root, 'focusout', INSPECTOR_AND_EDITOR_TARGETS.noteBodyField.selector, (_event, target) => {
      const field = target as HTMLTextAreaElement;
      if (field.dataset.noteResponse !== 'true') bindings.finish(field);
    }),
  );
  lifetime.add(
    delegate(root, 'click', INSPECTOR_AND_EDITOR_ACTIONS.saveNoteEdit.selector, (_event, target) => {
      bindings.save(target as HTMLElement);
    }),
  );
  return () => {
    lifetime.dispose();
  };
}

export interface MarkdownDetailsEditorBindings {
  /** A click or Enter/Space on the rendered Markdown began editing it. */
  readonly begin: (target: HTMLElement) => void;
  readonly input: (field: HTMLTextAreaElement) => void;
  /**
   * Focus left the Markdown source. `insideEditor` is true when it moved to another control of the
   * same MarkdownEditor, which keeps the edit open.
   */
  readonly focusLeave: (field: HTMLTextAreaElement, next: EventTarget | null, insideEditor: boolean) => void;
  /** The expand toggle, when the host shows it. */
  readonly toggleExpanded?: () => void;
}

/**
 * Wire MarkdownEditor edit entry, drafting, and focus-leave under `root`, plus the guard that keeps a
 * habitual double-click's repeat press inside the editor its first click opened.
 */
export function wireMarkdownDetailsEditor(root: HTMLElement, bindings: MarkdownDetailsEditorBindings): () => void {
  const lifetime = createScope();
  lifetime.add(
    delegateCapture(root, 'mousedown', '*', (event) => {
      if (repeatPressWouldLeaveNewEditor(event as MouseEvent, document.activeElement)) event.preventDefault();
    }),
  );
  lifetime.add(
    delegate(root, 'click', INSPECTOR_AND_EDITOR_ACTIONS.editMarkdown.selector, (event, target) => {
      if (clickBeginsMarkdownEdit(event as MouseEvent, target)) bindings.begin(target as HTMLElement);
    }),
  );
  lifetime.add(
    delegate(root, 'keydown', INSPECTOR_AND_EDITOR_ACTIONS.editMarkdown.selector, (event, target) => {
      if (!keyBeginsMarkdownEdit(event as KeyboardEvent, target)) return;
      event.preventDefault();
      bindings.begin(target as HTMLElement);
    }),
  );
  lifetime.add(
    delegate(root, 'input', INSPECTOR_AND_EDITOR_TARGETS.markdownSourceField.selector, (_event, target) => {
      bindings.input(target as HTMLTextAreaElement);
    }),
  );
  lifetime.add(
    delegate(root, 'focusout', INSPECTOR_AND_EDITOR_TARGETS.markdownSourceField.selector, (event, target) => {
      const next = (event as FocusEvent).relatedTarget,
        insideEditor =
          next instanceof Node && Boolean(target.closest('[data-component="markdown-editor"]')?.contains(next));
      bindings.focusLeave(target as HTMLTextAreaElement, next, insideEditor);
    }),
  );
  if (bindings.toggleExpanded) {
    const toggle = bindings.toggleExpanded;
    lifetime.add(
      delegate(root, 'click', TOGGLE_MARKDOWN_EXPANDED.selector, () => {
        toggle();
      }),
    );
  }
  return () => {
    lifetime.dispose();
  };
}
