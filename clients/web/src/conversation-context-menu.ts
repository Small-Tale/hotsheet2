import { signal } from 'kerfjs';

/** One transient AI conversation message menu, scoped to the originating connection. */
export interface ConversationContextMenuState {
  readonly connectionId: string;
  readonly messageIds: readonly string[];
  readonly x: number;
  readonly y: number;
}

export const conversationContextMenu = signal<ConversationContextMenuState | undefined>(undefined);

/** A selected target acts on the selection; any other target acts alone without changing it. */
export function conversationContextMessageIds(
  messageId: string,
  selectedMessageIds: readonly string[],
): readonly string[] {
  return selectedMessageIds.includes(messageId) ? selectedMessageIds : [messageId];
}
