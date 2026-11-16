/**
 * The mounted message composers of one AI conversation. The drawer chat and the conversation
 * dialog can be mounted together (the drawer also stays mounted through its hide transition), so a
 * send must address its own conversation's fields, never the first composer in the document
 * (HS2-YNCQ36). Each AIConversation root carries its connection id as `data-selection-id`.
 */
export function conversationComposers(root: ParentNode, connectionId: string): HTMLTextAreaElement[] {
  return [...root.querySelectorAll<HTMLTextAreaElement>('textarea[name="conversation-draft"]')].filter(
    (field) => field.closest<HTMLElement>('[data-selection-id]')?.dataset.selectionId === connectionId,
  );
}
