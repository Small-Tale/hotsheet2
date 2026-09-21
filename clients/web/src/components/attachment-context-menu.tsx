import './attachment-context-menu.css';

import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Clipboard, Copy, Download, ExternalLink, FolderOpen, Pencil, Trash2 } from 'lucide';

export type AttachmentContextMenuKind = 'item' | 'host';

export const ATTACHMENT_CONTEXT_MENU_HEIGHT = 274;

export interface AttachmentContextMenuProps {
  x: number;
  y: number;
  kind?: AttachmentContextMenuKind;
  revealLabel?: string;
}

const action = (
  id: string,
  label: string,
  icon: Parameters<typeof LucideIcon>[0]['icon'],
  iconName: string,
  className?: string,
) => (
  <wa-dropdown-item data-action="attachment-menu-action" data-item-id={id} class={className}>
    <span slot="icon">
      <LucideIcon icon={icon} name={iconName} />
    </span>
    {label}
  </wa-dropdown-item>
);

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
      style={`left:${x}px;top:${y}px`}
    >
      {action('open', 'Open', ExternalLink, 'external-link')}
      {action('download', 'Download', Download, 'download')}
      {action('copy-reference', 'Copy reference', Clipboard, 'clipboard')}
      {kind === 'item' && action('rename', 'Rename', Pencil, 'pencil')}
      {kind === 'host' && action('copy-path', 'Copy path', Copy, 'copy')}
      <hr />
      {action('reveal', revealLabel, FolderOpen, 'folder-open')}
      {action('remove', 'Remove', Trash2, 'trash-2', 'attachment-context-menu__danger')}
    </div>
  );
}
