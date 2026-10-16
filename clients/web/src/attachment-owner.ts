import type { FullTicket } from './api';
import type { TicketReaderFrame } from './ticket-reader-stack';

/**
 * Which ticket an attachment action acts on (HS2-W8VEMP, HS2-KKB9VF): a stacked linked reader
 * frame's own ticket, routed through that frame's project and provider capabilities, or the
 * workspace selection when the action did not come from a linked frame.
 */
export type AttachmentOwnerTarget =
  | {
      readonly kind: 'frame';
      readonly frame: TicketReaderFrame;
      readonly canUse: boolean;
      /** Existing attachments are editable; a provider that omits `attachment_edit` edits what it can add. */
      readonly canEdit: boolean;
      /** Attachments can be removed; a provider may support delete without other edits (HS2-51ADSG). */
      readonly canRemove: boolean;
    }
  | { readonly kind: 'workspace'; readonly projectId: string; readonly ticket: FullTicket };

/**
 * Resolve an attachment action's owner. An action from a frame that has since left the stack
 * resolves to nothing rather than falling through to the workspace selection, so a late menu or
 * gallery action can never rename, remove, or mark up an unrelated ticket.
 */
export function attachmentOwnerTarget(
  frameId: string | undefined,
  frames: readonly TicketReaderFrame[],
  workspace: { readonly projectId?: string; readonly ticket?: FullTicket | null },
): AttachmentOwnerTarget | undefined {
  if (frameId) {
    const frame = frames.find((item) => item.id === frameId);
    return frame
      ? {
          kind: 'frame',
          frame,
          canUse: frame.capabilities.attachments,
          canEdit: frame.capabilities.attachment_edit ?? frame.capabilities.attachments,
          canRemove:
            frame.capabilities.attachment_delete ??
            frame.capabilities.attachment_edit ??
            frame.capabilities.attachments,
        }
      : undefined;
  }
  return workspace.projectId && workspace.ticket
    ? { kind: 'workspace', projectId: workspace.projectId, ticket: workspace.ticket }
    : undefined;
}
