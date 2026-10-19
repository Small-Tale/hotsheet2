import { wireMessageComposers } from '@kerfjs/ui/wire-message-composers';
import { delegate, delegateCapture } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import { revealContextPopupMenu, viewportSafeContextMenuPosition } from '../context-menu-position';
import { toggleActivityGroup } from '../conversation-activity-disclosure';
import { conversationContextMenu } from '../conversation-context-menu';
import { type ConversationHistoryDirection, conversationHistoryDirection } from '../conversation-history';
import { type ConversationSelectionModifiers, conversationSelectionModifiers } from '../conversation-selection';
import { COMMANDS_AND_AI_ACTIONS, COMMANDS_AND_AI_TARGETS } from '../interaction-attrs/commands-and-ai';
import { isConversationSurfaceLifecycleEvent } from './ai-conversation';
import { wireContextMenuDismissal } from './context-menu-dismissal';
import { wireEmbeddedComposerClearance } from './embedded-composer-clearance';

/**
 * Side effects for the AIConversation surface (HS2-RWY78R). Production passes the conversation
 * controller; the UX demo passes fixture scenario changes, so both run the same handlers.
 */
export interface AiConversationBindings {
  /** The conversation surface itself (not a nested popup) finished hiding. */
  readonly onHidden: () => void;
  readonly save: () => void;
  /** Plain, toggle (Cmd on Apple platforms, Ctrl elsewhere), or Shift-extend pick (HS2-WV88W3). */
  readonly pickMessage: (connectionId: string, messageId: string, modifiers: ConversationSelectionModifiers) => void;
  readonly contextMessageIds: (connectionId: string, messageId: string) => readonly string[];
  readonly copyMessages: (connectionId: string, messageIds: readonly string[]) => void;
  readonly downloadMessages: (connectionId: string, messageIds: readonly string[]) => void;
  readonly setDraft: (value: string) => void;
  /**
   * Step the composer through this conversation's sent messages (HS2-AJQKK0). Returns the text
   * to show, or `undefined` when there is nowhere to go and the arrow key keeps its default.
   */
  readonly recallHistory: (direction: ConversationHistoryDirection, currentValue: string) => string | undefined;
  readonly send: () => void;
  readonly stop: () => void;
  readonly selectProvider: (id: string) => void;
  readonly selectModel: (model: string) => void;
  readonly selectEffort: (effort: string) => void;
  /** Switch the chat's permission mode (HS2-NHRDMW). */
  readonly selectPermissionMode?: (mode: string) => void;
  readonly openManualModel: () => void;
}

const valueOf = (target: Element) => (target as HTMLElement).dataset.value ?? '';

const NESTED_MESSAGE_CONTROL = 'a,button,input,select,textarea';

function pickedMessage(target: Element) {
  return {
    connectionId: target.closest<HTMLElement>('[data-selection-id]')?.dataset.selectionId,
    messageId: (target as HTMLElement).dataset.messageId,
  };
}

/** Wire the AIConversation surface, composer, and AI menus under `root`. */
export function wireAiConversation(root: HTMLElement, bindings: AiConversationBindings): () => void {
  const lifetime = createScope();
  const openMessageMenu = (target: Element, x: number, y: number) => {
    const { connectionId, messageId } = pickedMessage(target);
    if (!connectionId || !messageId) return;
    const messageIds = bindings.contextMessageIds(connectionId, messageId);
    if (!messageIds.length) return;
    const anchor = viewportSafeContextMenuPosition(x, y, window.innerWidth, window.innerHeight, {
      width: 240,
      height: 112,
    });
    conversationContextMenu.value = { connectionId, messageIds: [...messageIds], ...anchor };
    revealContextPopupMenu('conversation', root);
  };
  lifetime.add(
    wireContextMenuDismissal(root, [
      {
        surface: 'conversation',
        isOpen: () => Boolean(conversationContextMenu.value),
        stateToken: () => conversationContextMenu.value,
        close: () => {
          conversationContextMenu.value = undefined;
        },
        hide: true,
      },
    ]),
  );
  lifetime.add(
    delegateCapture(
      root,
      'kui-request-close',
      COMMANDS_AND_AI_TARGETS.aiConversationDialog.selector,
      (event, target) => {
        if (isConversationSurfaceLifecycleEvent(event, target)) {
          conversationContextMenu.value = undefined;
          bindings.onHidden();
        }
      },
    ),
  );
  lifetime.add(
    delegate(root, 'click', COMMANDS_AND_AI_ACTIONS.saveConversation.selector, () => {
      bindings.save();
    }),
  );
  lifetime.add(
    delegate(root, 'contextmenu', COMMANDS_AND_AI_ACTIONS.pickConversationMessage.selector, (event, target) => {
      if ((event.target as Element).closest(NESTED_MESSAGE_CONTROL) || window.getSelection()?.toString()) return;
      event.preventDefault();
      const pointer = event as MouseEvent;
      openMessageMenu(target, pointer.clientX, pointer.clientY);
    }),
  );
  lifetime.add(
    delegate(root, 'click', '[data-conversation-context-action]', (_event, target) => {
      const request = conversationContextMenu.value;
      if (!request) return;
      conversationContextMenu.value = undefined;
      if ((target as HTMLElement).dataset.conversationContextAction === 'copy')
        bindings.copyMessages(request.connectionId, request.messageIds);
      else bindings.downloadMessages(request.connectionId, request.messageIds);
    }),
  );
  // Shift- and Cmd/Ctrl-clicks pick messages instead of extending the page's text selection.
  lifetime.add(
    delegateCapture(root, 'mousedown', COMMANDS_AND_AI_ACTIONS.pickConversationMessage.selector, (event) => {
      const modifiers = conversationSelectionModifiers(event as MouseEvent);
      if ((modifiers.extend || modifiers.toggle) && !(event.target as Element).closest(NESTED_MESSAGE_CONTROL))
        event.preventDefault();
    }),
  );
  lifetime.add(
    delegate(root, 'click', COMMANDS_AND_AI_ACTIONS.pickConversationMessage.selector, (event, target) => {
      const modifiers = conversationSelectionModifiers(event as MouseEvent);
      if ((event.target as Element).closest(NESTED_MESSAGE_CONTROL)) return;
      // A plain click that ends a text drag is reading, not picking; a modified click is always a pick.
      if (!modifiers.extend && !modifiers.toggle && window.getSelection()?.toString()) return;
      const { connectionId, messageId } = pickedMessage(target);
      if (connectionId && messageId) bindings.pickMessage(connectionId, messageId, modifiers);
    }),
  );
  // Kerf activates a focused message on plain Enter/Space as a click without modifiers; Shift or
  // Cmd/Ctrl with Enter/Space is handled here first and cancelled so Kerf skips it (KF-45RNDS).
  lifetime.add(
    delegateCapture(root, 'keydown', COMMANDS_AND_AI_ACTIONS.pickConversationMessage.selector, (event, target) => {
      const keyboard = event as KeyboardEvent,
        modifiers = conversationSelectionModifiers(keyboard);
      if (event.target !== target || keyboard.altKey || !(modifiers.extend || modifiers.toggle)) return;
      if (keyboard.key !== 'Enter' && keyboard.key !== ' ' && keyboard.key !== 'Spacebar') return;
      event.preventDefault();
      if (keyboard.repeat) return;
      const { connectionId, messageId } = pickedMessage(target);
      if (connectionId && messageId) bindings.pickMessage(connectionId, messageId, modifiers);
    }),
  );
  lifetime.add(
    delegate(root, 'keydown', COMMANDS_AND_AI_ACTIONS.pickConversationMessage.selector, (event, target) => {
      const keyboard = event as KeyboardEvent;
      if (event.target !== target || !(keyboard.key === 'ContextMenu' || (keyboard.shiftKey && keyboard.key === 'F10')))
        return;
      event.preventDefault();
      if (keyboard.repeat) return;
      const rect = target.getBoundingClientRect();
      openMessageMenu(target, rect.left + 20, rect.top + 20);
    }),
  );
  // A click anywhere in an activity group toggles it collapsed/expanded, except on its nested
  // feedback controls or links, or while the person is selecting its text (HS2-Y3SGH3). The header
  // toggle is a Kerf ListHeader button, so Enter and Space reach this handler as a click.
  lifetime.add(
    delegate(root, 'click', COMMANDS_AND_AI_ACTIONS.toggleConversationActivity.selector, (event, target) => {
      const control = (event.target as Element).closest('a,button,wa-button,input,select,textarea');
      if (control && !control.matches(COMMANDS_AND_AI_ACTIONS.toggleConversationActivity.selector)) return;
      if (!control && window.getSelection()?.toString()) return;
      const groupId =
        (target as HTMLElement).dataset.activityGroup ??
        target.closest<HTMLElement>('[data-activity-group]')?.dataset.activityGroup;
      if (groupId)
        toggleActivityGroup(target.closest<HTMLElement>('[data-selection-id]')?.dataset.selectionId, groupId);
    }),
  );
  lifetime.add(
    delegate(root, 'input', COMMANDS_AND_AI_TARGETS.conversationDraftField.selector, (_event, target) => {
      bindings.setDraft((target as HTMLTextAreaElement).value);
    }),
  );
  // Up/Down on the composer's first/last line browse sent messages like a shell (HS2-AJQKK0).
  lifetime.add(
    delegate(root, 'keydown', COMMANDS_AND_AI_TARGETS.conversationDraftField.selector, (event, target) => {
      const field = target as HTMLTextAreaElement,
        direction = conversationHistoryDirection(event as KeyboardEvent, field);
      if (!direction || field.disabled || field.readOnly) return;
      const value = bindings.recallHistory(direction, field.value);
      if (value === undefined) return;
      event.preventDefault();
      field.value = value;
      field.setSelectionRange(value.length, value.length);
    }),
  );
  // Kerf owns Enter-to-send (IME, empty, and auto-repeat guards) for the submitOnEnter MessageComposer.
  lifetime.add(wireMessageComposers(root));
  // The drawer chat's transcript clears its floating composer by the measured height (HS2-W0KJVQ).
  lifetime.add(wireEmbeddedComposerClearance(root));
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
    delegate(root, 'click', COMMANDS_AND_AI_ACTIONS.selectConversationPermissionMode.selector, (_event, target) => {
      bindings.selectPermissionMode?.(valueOf(target));
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
