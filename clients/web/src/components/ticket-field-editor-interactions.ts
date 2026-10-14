import { isContentItemActivation } from '@kerfjs/ui/wire-content-items';
import { delegate } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import { INSPECTOR_AND_EDITOR_ACTIONS, INSPECTOR_AND_EDITOR_TARGETS } from '../interaction-attrs/inspector-and-editor';
import { normalizeTicketTitleField, ticketTitleKeyFinishesEdit } from '../ticket-title-editing';
import { TAG_CHIP_REMOVE_ACTION } from './tag-chip';

/**
 * Side effects for the inspector and reader title, tag, blocked-reason, and feedback-choice editors
 * (HS2-KAWNER). Production passes the selected-ticket state and autosaves; the UX demo passes fixture
 * callbacks, so both run the same event handling and key rules.
 */

/** The ticket title editor: double-click or Enter/Space on the heading begins an edit. */
export interface TicketTitleEditorBindings {
  readonly begin: (trigger: Element) => void;
  /** The title field changed; the value is already collapsed to one line (HS2-98ZVPE). */
  readonly input: (value: string, field: HTMLTextAreaElement) => void;
  /** Focus left the title field; Enter finishes through this same blur. */
  readonly finish: (field: Element) => void;
}

/** Wire the title editor under `root`. */
export function wireTicketTitleEditor(root: HTMLElement, bindings: TicketTitleEditorBindings): () => void {
  const lifetime = createScope();
  lifetime.add(
    delegate(root, 'dblclick', INSPECTOR_AND_EDITOR_ACTIONS.editTicketTitle.selector, (_event, target) => {
      bindings.begin(target);
    }),
  );
  lifetime.add(
    delegate(root, 'keydown', INSPECTOR_AND_EDITOR_ACTIONS.editTicketTitle.selector, (event, target) => {
      if (!['Enter', ' '].includes((event as KeyboardEvent).key)) return;
      event.preventDefault();
      bindings.begin(target);
    }),
  );
  lifetime.add(
    delegate(root, 'input', INSPECTOR_AND_EDITOR_TARGETS.ticketTitleField.selector, (_event, target) => {
      // The wrapping editor is a textarea, but a title stays one line: pasted breaks collapse (HS2-98ZVPE).
      const field = target as HTMLTextAreaElement;
      bindings.input(normalizeTicketTitleField(field), field);
    }),
  );
  lifetime.add(
    delegate(root, 'keydown', INSPECTOR_AND_EDITOR_TARGETS.ticketTitleField.selector, (event, target) => {
      // Enter finishes the edit through the focusout autosave instead of inserting a newline.
      if (!ticketTitleKeyFinishesEdit(event as KeyboardEvent)) return;
      event.preventDefault();
      (target as HTMLTextAreaElement).blur();
    }),
  );
  lifetime.add(
    delegate(root, 'focusout', INSPECTOR_AND_EDITOR_TARGETS.ticketTitleField.selector, (_event, target) => {
      bindings.finish(target);
    }),
  );
  return () => {
    lifetime.dispose();
  };
}

/** The tag editor: Enter or comma adds the typed tag, blur adds any pending text and flushes. */
export interface TicketTagEditorBindings {
  /** Add `value` (the input is cleared afterwards). */
  readonly add: (value: string) => void;
  readonly flush: () => void;
  readonly remove: (tag: string) => void;
}

/** Wire the tag editor and its chips' remove buttons under `root`. */
export function wireTicketTagEditor(root: HTMLElement, bindings: TicketTagEditorBindings): () => void {
  const lifetime = createScope();
  const addFromInput = (input: HTMLInputElement) => {
    const value = input.value;
    input.value = '';
    bindings.add(value);
  };
  lifetime.add(
    delegate(root, 'keydown', INSPECTOR_AND_EDITOR_TARGETS.ticketTagInputField.selector, (event, target) => {
      if (!['Enter', ','].includes((event as KeyboardEvent).key)) return;
      event.preventDefault();
      addFromInput(target as HTMLInputElement);
    }),
  );
  lifetime.add(
    delegate(root, 'focusout', INSPECTOR_AND_EDITOR_TARGETS.ticketTagInputField.selector, (_event, target) => {
      if ((target as HTMLInputElement).value.trim()) addFromInput(target as HTMLInputElement);
      bindings.flush();
    }),
  );
  lifetime.add(
    delegate(root, 'click', `[data-action="${TAG_CHIP_REMOVE_ACTION}"]`, (_event, target) => {
      const tag = target.closest<HTMLElement>('[data-component="chip"]')?.dataset.itemId;
      if (tag) bindings.remove(tag);
    }),
  );
  return () => {
    lifetime.dispose();
  };
}

/** How a blocked-reason edit began: the explicit Edit action, a double-click, or Enter/Space. */
export type BlockedReasonEditTrigger = 'action' | 'dblclick' | 'key';

export interface BlockedReasonEditorBindings {
  readonly begin: (target: Element, trigger: BlockedReasonEditTrigger) => void;
  readonly input: (value: string, field: HTMLTextAreaElement) => void;
  readonly finish: (field: Element) => void;
}

/** Wire the blocked-reason editor under `root`. */
export function wireBlockedReasonEditor(root: HTMLElement, bindings: BlockedReasonEditorBindings): () => void {
  const lifetime = createScope();
  lifetime.add(
    delegate(root, 'click', INSPECTOR_AND_EDITOR_ACTIONS.editBlockedReason.selector, (_event, target) => {
      bindings.begin(target, 'action');
    }),
  );
  lifetime.add(
    delegate(root, 'dblclick', INSPECTOR_AND_EDITOR_TARGETS.editBlockedReason.selector, (_event, target) => {
      bindings.begin(target, 'dblclick');
    }),
  );
  lifetime.add(
    delegate(root, 'keydown', INSPECTOR_AND_EDITOR_TARGETS.editBlockedReason.selector, (event, target) => {
      if (!['Enter', ' '].includes((event as KeyboardEvent).key)) return;
      event.preventDefault();
      bindings.begin(target, 'key');
    }),
  );
  lifetime.add(
    delegate(root, 'input', INSPECTOR_AND_EDITOR_TARGETS.blockedReasonField.selector, (_event, target) => {
      const field = target as HTMLTextAreaElement;
      bindings.input(field.value, field);
    }),
  );
  lifetime.add(
    delegate(root, 'focusout', INSPECTOR_AND_EDITOR_TARGETS.blockedReasonField.selector, (_event, target) => {
      bindings.finish(target);
    }),
  );
  return () => {
    lifetime.dispose();
  };
}

/** Pointer or keyboard modifiers that shape a feedback-choice selection change. */
export interface FeedbackChoiceModifiers {
  readonly additive: boolean;
  readonly range: boolean;
}

export interface FeedbackChoicesBindings {
  readonly toggle: (noteId: string, choiceId: string, modifiers: FeedbackChoiceModifiers) => void;
}

/**
 * Wire pointer toggling of feedback choices under `root`. The choices are Kerf
 * multi-select ContentItems: `wireContentItems` (registered once app-wide)
 * turns Enter/Space into a click, and nested links or gallery buttons keep
 * their own action because `isContentItemActivation` rejects them.
 */
export function wireFeedbackChoices(root: HTMLElement, bindings: FeedbackChoicesBindings): () => void {
  const lifetime = createScope();
  lifetime.add(
    delegate(root, 'click', INSPECTOR_AND_EDITOR_ACTIONS.toggleFeedbackChoice.selector, (event, target) => {
      const item = target as HTMLElement,
        { noteId, choiceId } = item.dataset;
      if (!noteId || !choiceId || !isContentItemActivation(event, item)) return;
      const mouse = event as MouseEvent;
      bindings.toggle(noteId, choiceId, { additive: mouse.metaKey || mouse.ctrlKey, range: mouse.shiftKey });
    }),
  );
  return () => {
    lifetime.dispose();
  };
}
