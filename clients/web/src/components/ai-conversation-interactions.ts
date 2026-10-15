import { wireComposers } from '@kerfjs/ui/wire-composers';
import { delegate, delegateCapture } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import { toggleActivityGroup } from '../conversation-activity-disclosure';
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
  // A click anywhere in an activity group toggles it collapsed/expanded, except on its nested
  // feedback controls or links, or while the person is selecting its text (HS2-Y3SGH3). The header
  // toggle is a native button, so Enter and Space reach this handler as a click.
  lifetime.add(
    delegate(root, 'click', COMMANDS_AND_AI_ACTIONS.toggleConversationActivity.selector, (event, target) => {
      const control = (event.target as Element).closest('a,button,wa-button,input,select,textarea');
      if (control && !control.matches('.ai-conversation__activity-toggle')) return;
      if (!control && window.getSelection()?.toString()) return;
      const groupId = (target as HTMLElement).dataset.activityGroup;
      if (groupId)
        toggleActivityGroup(target.closest<HTMLElement>('[data-selection-id]')?.dataset.selectionId, groupId);
    }),
  );
  lifetime.add(
    delegate(root, 'input', COMMANDS_AND_AI_TARGETS.conversationDraftField.selector, (_event, target) => {
      bindings.setDraft((target as HTMLTextAreaElement).value);
    }),
  );
  // Kerf owns Enter-to-send (IME, empty, and auto-repeat guards) for the submitOnEnter Composer.
  lifetime.add(wireComposers(root));
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
