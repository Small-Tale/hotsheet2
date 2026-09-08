import './attachment-context-menu.css';

import { Clipboard, Copy, Download, ExternalLink, FolderOpen, Trash2 } from 'lucide';

import { LucideIcon } from './lucide-icon';
import { MenuItem } from './menu-item';

export type AttachmentContextMenuKind = 'item' | 'host';

export interface AttachmentContextMenuProps {
  x: number;
  y: number;
  kind?: AttachmentContextMenuKind;
  revealLabel?: string;
}

const action = (id:string,label:string,icon:Parameters<typeof LucideIcon>[0]['icon'],iconName:string,className?:string) =>
  <MenuItem role="menuitem" action="attachment-menu-action" itemId={id} label={label} icon={<LucideIcon icon={icon} name={iconName}/>} className={className}/>;

/** Shared attachment actions opened from item ellipses, item right-click, and gallery media. */
export function AttachmentContextMenu({x,y,kind='item',revealLabel='Show in file manager'}:AttachmentContextMenuProps){
  return <div class="attachment-context-menu" data-component="attachment-context-menu" data-kind={kind} role="menu" aria-label="Attachment actions" style={`left:${x}px;top:${y}px`}>
    {action('open','Open',ExternalLink,'external-link')}
    {action('download','Download',Download,'download')}
    {action('copy-reference','Copy reference',Clipboard,'clipboard')}
    {kind==='host'?<>{action('copy-path','Copy path',Copy,'copy')}<hr/>{action('reveal',revealLabel,FolderOpen,'folder-open')}</>:action('remove','Remove',Trash2,'trash-2','attachment-context-menu__danger')}
  </div>;
}
