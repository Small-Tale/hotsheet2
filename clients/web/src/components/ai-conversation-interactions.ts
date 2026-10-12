import { delegate, delegateCapture } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import { COMMANDS_AND_AI_ACTIONS, COMMANDS_AND_AI_TARGETS } from '../interaction-attrs/commands-and-ai';
import { isConversationSurfaceLifecycleEvent } from './ai-conversation';

/**
 * Side effects for the AIConversation surface (HS2-RWY78R). Production passes the conversation
 * controller; the UX demo passes fixture scenario changes, so both run the same handlers.
 */
export interface AiConversationBindings {
  /** The conversation surface itself (not a nested popup) finished hiding. */
  readonly onHidden: () => void;
  readonly save: () => void;
  readonly pickMessage: (connectionId: string, messageId: string) => void;
  readonly setDraft: (value: string) => void;
  readonly send: () => void;
  readonly stop: () => void;
  readonly selectProvider: (id: string) => void;
  readonly selectModel: (model: string) => void;
  readonly selectEffort: (effort: string) => void;
  readonly openManualModel: () => void;
}

const valueOf = (target: Element) => (target as HTMLElement).dataset.value ?? '';

/** Wire the AIConversation surface, composer, and AI menus under `root`. */
export function wireAiConversation(root: HTMLElement, bindings: AiConversationBindings): () => void {
  const lifetime = createScope();
  lifetime.add(
    delegateCapture(root, 'wa-hide', COMMANDS_AND_AI_TARGETS.aiConversation.selector, (event, target) => {
      if (isConversationSurfaceLifecycleEvent(event, target)) bindings.onHidden();
    }),
  );
  lifetime.add(
    delegate(root, 'click', COMMANDS_AND_AI_ACTIONS.saveConversation.selector, () => {
      bindings.save();
    }),
  );
  lifetime.add(
    delegate(root, 'click', COMMANDS_AND_AI_ACTIONS.pickConversationMessage.selector, (event, target) => {
      if ((event.target as Element).closest('a,button,input,select,textarea') || window.getSelection()?.toString())
        return;
      const connectionId = target.closest<HTMLElement>('[data-selection-id]')?.dataset.selectionId,
        messageId = (target as HTMLElement).dataset.messageId;
      if (connectionId && messageId) bindings.pickMessage(connectionId, messageId);
    }),
  );
  lifetime.add(
    delegate(root, 'input', COMMANDS_AND_AI_TARGETS.conversationDraftField.selector, (_event, target) => {
      bindings.setDraft((target as HTMLTextAreaElement).value);
    }),
  );
  lifetime.add(
    delegate(root, 'keydown', COMMANDS_AND_AI_TARGETS.conversationDraftField.selector, (event, target) => {
      const keyboard = event as KeyboardEvent;
      if (keyboard.key !== 'Enter' || keyboard.shiftKey || keyboard.isComposing) return;
      keyboard.preventDefault();
      target.closest<HTMLFormElement>('form')?.requestSubmit();
    }),
  );
  lifetime.add(
    delegate(root, 'submit', COMMANDS_AND_AI_ACTIONS.sendConversationTurn.selector, (event) => {
      event.preventDefault();
      bindings.send();
    }),
  );
  lifetime.add(
    delegate(root, 'click', COMMANDS_AND_AI_ACTIONS.stopConversation.selector, () => {
      bindings.stop();
    }),
  );
  lifetime.add(
    delegate(root, 'click', COMMANDS_AND_AI_ACTIONS.selectConversationProvider.selector, (_event, target) => {
      bindings.selectProvider(valueOf(target));
    }),
  );
  lifetime.add(
    delegate(root, 'click', COMMANDS_AND_AI_ACTIONS.selectConversationModel.selector, (_event, target) => {
      bindings.selectModel(valueOf(target));
    }),
  );
  lifetime.add(
    delegate(root, 'click', COMMANDS_AND_AI_ACTIONS.selectConversationEffort.selector, (_event, target) => {
      bindings.selectEffort(valueOf(target));
    }),
  );
  lifetime.add(
    delegate(root, 'click', COMMANDS_AND_AI_ACTIONS.openConversationManualModel.selector, () => {
      bindings.openManualModel();
    }),
  );
  return () => {
    lifetime.dispose();
  };
}
