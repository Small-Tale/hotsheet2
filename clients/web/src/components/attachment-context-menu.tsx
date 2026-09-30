import './attachment-context-menu.css';

import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { PopupMenu, type PopupMenuItem } from '@kerfjs/ui/popup-menu';
import { Clipboard, Copy, Download, ExternalLink, FolderOpen, Pencil, Trash2 } from 'lucide';

import { contextPopupMenuAnchor } from '../context-menu-position';

export type AttachmentContextMenuKind = 'item' | 'host';

export const ATTACHMENT_CONTEXT_MENU_HEIGHT = 274;

export interface AttachmentContextMenuProps {
  x: number;
  y: number;
  kind?: AttachmentContextMenuKind;
  revealLabel?: string;
}

const item = (
  id: string,
  label: string,
  icon: Parameters<typeof LucideIcon>[0]['icon'],
  iconName: string,
  tone: 'default' | 'danger' = 'default',
): PopupMenuItem => ({
  label,
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
}: AttachmentContextMenuProps) {
  return (
    <div
      class="attachment-context-menu"
      data-component="attachment-context-menu"
      data-kind={kind}
      role="menu"
      aria-label="Attachment actions"
      {...contextPopupMenuAnchor(x, y)}
    >
      <PopupMenu
        context
        label="Attachment actions"
        rootAttributes={{ 'data-context-menu': 'attachment' }}
        items={[
          item('open', 'Open', ExternalLink, 'external-link'),
          item('download', 'Download', Download, 'download'),
          item('copy-reference', 'Copy reference', Clipboard, 'clipboard'),
          ...(kind === 'item' ? [item('rename', 'Rename', Pencil, 'pencil')] : []),
          ...(kind === 'host' ? [item('copy-path', 'Copy path', Copy, 'copy')] : []),
          { kind: 'divider' },
          item('reveal', revealLabel, FolderOpen, 'folder-open'),
          item('remove', 'Remove', Trash2, 'trash-2', 'danger'),
        ]}
      />
    </div>
  );
}
