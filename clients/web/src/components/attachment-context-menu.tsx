import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { PopupMenu, type PopupMenuItem } from '@kerfjs/ui/popup-menu';
import { Clipboard, Copy, Download, ExternalLink, FolderOpen, Pencil, Trash2 } from 'lucide';

import { hostActionsAvailableHere, hostOnlyDisabledReason } from '../host-only-actions';
import { ContextMenuAnchor } from './context-menu-anchor';

export type AttachmentContextMenuKind = 'item' | 'host';

export const ATTACHMENT_CONTEXT_MENU_HEIGHT = 274;

export interface AttachmentContextMenuProps {
  x: number;
  y: number;
  kind?: AttachmentContextMenuKind;
  revealLabel?: string;
  /** False on a remote client: Open and reveal render disabled with an explanation (HS2-TYS5SS). */
  hostActionsAvailable?: boolean;
}

const item = (
  id: string,
  label: string,
  icon: Parameters<typeof LucideIcon>[0]['icon'],
  iconName: string,
  tone: 'default' | 'danger' = 'default',
  disabledReason?: string,
): PopupMenuItem => ({
  label,
  ...(disabledReason ? { disabled: true, disabledReason } : {}),
  action: 'attachment-menu-action',
  tone,
  icon: <LucideIcon icon={icon} name={iconName} />,
  attributes: { 'data-item-id': id },
});

/** Shared attachment actions opened from item ellipses, item right-click, and gallery media. */
export function AttachmentContextMenu({
  x,
  y,
  kind = 'item',
  revealLabel = 'Show in file manager',
  hostActionsAvailable = hostActionsAvailableHere(),
}: AttachmentContextMenuProps) {
  return (
    <ContextMenuAnchor
      x={x}
      y={y}
      component="attachment-context-menu"
      label="Attachment actions"
      attributes={{ 'data-kind': kind }}
    >
      <PopupMenu
        context
        label="Attachment actions"
        rootAttributes={{ 'data-context-menu': 'attachment' }}
        items={[
          item(
            'open',
            'Open',
            ExternalLink,
            'external-link',
            'default',
            hostOnlyDisabledReason('open-file', hostActionsAvailable),
          ),
          item('download', 'Download', Download, 'download'),
          item('copy-reference', 'Copy reference', Clipboard, 'clipboard'),
          ...(kind === 'item' ? [item('rename', 'Rename', Pencil, 'pencil')] : []),
          ...(kind === 'host' ? [item('copy-path', 'Copy path', Copy, 'copy')] : []),
          { kind: 'divider' },
          item(
            'reveal',
            revealLabel,
            FolderOpen,
            'folder-open',
            'default',
            hostOnlyDisabledReason('reveal-file', hostActionsAvailable),
          ),
          item('remove', 'Remove', Trash2, 'trash-2', 'danger'),
        ]}
      />
    </ContextMenuAnchor>
  );
}
